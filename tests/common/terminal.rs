//! Drive a native PTY by observed output, with bounded waits and child cleanup.

use super::Isolation;
use portable_pty::{
    Child, CommandBuilder, MasterPty, PtySize, native_pty_system,
};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(50);

pub fn terminal(
    isolation: &Isolation,
    args: &[&str],
    api: &str,
    steps: &[(&str, &str)],
) -> String {
    terminal_in(isolation, args, api, steps, &isolation.config_dir)
}

pub fn terminal_in(
    isolation: &Isolation,
    args: &[&str],
    api: &str,
    steps: &[(&str, &str)],
    cwd: &std::path::Path,
) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut transcript = Vec::new();
    let mut command = command(isolation, args, api);
    command.cwd(cwd);
    let result = Session::open(command).and_then(|mut session| {
        session.drive(steps, deadline, &mut transcript)
    });
    assert!(
        result.is_ok(),
        "PTY session failed: {result:?}\n{}",
        String::from_utf8_lossy(&transcript)
    );
    String::from_utf8(transcript).expect("UTF-8 terminal transcript")
}

fn command(isolation: &Isolation, args: &[&str], api: &str) -> CommandBuilder {
    let template = isolation.command(args);
    let mut command = CommandBuilder::new(template.get_program());
    command.args(template.get_args());
    // CommandBuilder may augment inherited environment variables (including
    // PATH on Windows). Copy the actual process environment before overrides.
    command.env_clear();
    for (name, value) in std::env::vars_os() {
        command.env(name, value);
    }
    for (name, value) in template.get_envs() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        }
    }
    command.env("LABELDECK_API", api);
    command.env("LABELDECK_TOKEN", "test-token");
    command.env("NO_COLOR", "1");
    command.env("TERM", "xterm");
    command.cwd(&isolation.config_dir);
    command
}

struct Session {
    child: Box<dyn Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    reader: Option<JoinHandle<()>>,
    output: Receiver<io::Result<Vec<u8>>>,
    // Keep recv_timeout usable after the reader sends EOF and exits.
    _sender: mpsc::Sender<io::Result<Vec<u8>>>,
    exited: bool,
}

impl Session {
    fn open(command: CommandBuilder) -> io::Result<Self> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;
        let reader =
            pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(io::Error::other)?;
        drop(pair.slave);
        let (sender, output) = mpsc::channel();
        let mut session = Self {
            child,
            master: Some(pair.master),
            writer: Some(writer),
            reader: None,
            output,
            _sender: sender.clone(),
            exited: false,
        };
        session.reader = Some(thread::Builder::new().spawn(move || {
            read_output(reader, sender);
        })?);
        Ok(session)
    }

    fn drive(
        &mut self,
        steps: &[(&str, &str)],
        deadline: Instant,
        transcript: &mut Vec<u8>,
    ) -> io::Result<()> {
        let mut script = Script::default();
        let mut eof = false;
        let mut status = None;
        while !eof || status.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "terminal driver timed out; next step: {:?}",
                        steps.get(script.step)
                    ),
                ));
            }
            match self.output.recv_timeout(remaining.min(POLL)) {
                Ok(chunk) => {
                    let chunk = chunk?;
                    eof = chunk.is_empty();
                    transcript.extend_from_slice(&chunk);
                    script.observe(
                        &chunk,
                        steps,
                        self.writer.as_mut().unwrap(),
                    )?;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(error) => return Err(io::Error::other(error)),
            }
            if status.is_none() {
                status = self.child.try_wait()?;
                if status.is_some() {
                    self.exited = true;
                    // ConPTY retains its output pipe until the console closes.
                    // The reader keeps draining while ClosePseudoConsole runs.
                    self.master.take();
                }
            }
        }
        script.finish(steps, &status.unwrap(), transcript)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.exited {
            // Native Unix children are std::process::Child. Use its hard kill
            // rather than portable-pty's cloned killer, which only sends HUP.
            let child: &mut dyn Child = self.child.as_mut();
            if let Some(child) = child.downcast_mut::<std::process::Child>() {
                let _ = child.kill();
            } else {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
        self.writer.take();
        self.master.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_output(
    mut reader: Box<dyn Read + Send>,
    sender: mpsc::Sender<io::Result<Vec<u8>>>,
) {
    let mut buffer = [0; 65536];
    loop {
        let chunk = match reader.read(&mut buffer) {
            Ok(size) => Ok(buffer[..size].to_vec()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(error) => Err(error),
        };
        let done = chunk.as_ref().map_or(true, Vec::is_empty);
        if sender.send(chunk).is_err() || done {
            break;
        }
    }
}

#[derive(Default)]
struct Script {
    pending: Vec<u8>,
    query: Vec<u8>,
    step: usize,
}

impl Script {
    fn finish(
        &self,
        steps: &[(&str, &str)],
        status: &portable_pty::ExitStatus,
        transcript: &[u8],
    ) -> io::Result<()> {
        if let Some((marker, _)) = steps.get(self.step) {
            return Err(io::Error::other(format!(
                "unreached terminal step: {marker}"
            )));
        }
        if !status.success() {
            return Err(io::Error::other(format!(
                "child exit status {status}"
            )));
        }
        if !contains(transcript, b"\x1b[?1049l") {
            return Err(io::Error::other("alternate screen was not restored"));
        }
        Ok(())
    }

    fn observe(
        &mut self,
        chunk: &[u8],
        steps: &[(&str, &str)],
        writer: &mut dyn Write,
    ) -> io::Result<()> {
        self.pending.extend_from_slice(chunk);
        self.query.extend_from_slice(chunk);
        while let Some(index) =
            self.query.windows(4).position(|w| w == b"\x1b[6n")
        {
            writer.write_all(b"\x1b[1;1R")?;
            self.query.drain(..index + 4);
        }
        // Keep only a possible partial query across read boundaries.
        self.query.drain(..self.query.len().saturating_sub(3));
        if let Some((marker, keys)) = steps.get(self.step)
            && contains(&self.pending, marker.as_bytes())
        {
            self.pending.clear();
            writer.write_all(keys.as_bytes())?;
            self.step += 1;
        }
        writer.flush()
    }
}

fn contains(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|window| window == marker)
}

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;
