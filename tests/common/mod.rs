//! Shared test support: a deterministic in-process mock of the GitHub
//! REST API, used by integration tests so no test ever contacts GitHub.

#[cfg(unix)]
pub mod terminal;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

use parking_lot::Mutex;
#[derive(Clone, Debug)]
pub struct Expectation {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Expectation {
    pub fn get(path: &str) -> Self {
        Self::method("GET", path)
    }

    pub fn post(path: &str) -> Self {
        Self::method("POST", path)
    }

    pub fn patch(path: &str) -> Self {
        Self::method("PATCH", path)
    }

    pub fn delete(path: &str) -> Self {
        Self::method("DELETE", path)
    }

    fn method(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            status: 200,
            headers: Vec::new(),
            body: String::new(),
        }
    }

    pub fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn body(mut self, body: &str) -> Self {
        self.body = body.to_string();
        self
    }

    /// A successful page of repository labels.
    pub fn labels_page(
        self,
        labels_json: &str,
        next_page: Option<&str>,
    ) -> Self {
        let mut page = self;
        if let Some(next) = next_page {
            page = page.header("link", &format!("<{next}>; rel=\"next\""));
        }
        page.body(labels_json)
    }
}

/// An HTTP request the mock received.
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Default, Debug)]
struct ServerState {
    queue: VecDeque<Expectation>,
    requests: Vec<RecordedRequest>,
    mismatches: Vec<String>,
}

/// A running mock server. Dropping it is fine; its listener thread dies
/// with the test process.
pub struct MockGitHub {
    base_url: String,
    state: Arc<Mutex<ServerState>>,
}

impl MockGitHub {
    /// The API base URL to hand to
    /// [`labeldeck::github::GitHubClient::with_base_url`].
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Queue another scripted response after the server has started, so
    /// tests can reference the server's own dynamically assigned URL.
    pub fn expect(&self, expectation: Expectation) {
        self.state.lock().queue.push_back(expectation);
    }

    /// Every request received, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.lock().requests.clone()
    }

    /// Panic (with details) unless every scripted expectation was
    /// consumed by a matching request and nothing unexpected arrived.
    pub fn assert_satisfied(&self) {
        let state = self.state.lock();
        let mut problems = Vec::new();
        if !state.mismatches.is_empty() {
            problems
                .push(format!("mismatched requests: {:#?}", state.mismatches));
        }
        if problems.is_empty() {
            return;
        }
        problems.push(format!(
            "requests received: {:#?}",
            state
                .requests
                .iter()
                .map(|r| (&r.method, &r.path))
                .collect::<Vec<_>>()
        ));
        if !state.queue.is_empty() {
            problems.push(format!(
                "unmet expectations: {:#?}",
                state
                    .queue
                    .iter()
                    .map(|e| (&e.method, &e.path))
                    .collect::<Vec<_>>()
            ));
        }
        assert!(problems.is_empty(), "{}", problems.join("; "));
    }
}

/// Start a mock GitHub API on a random localhost port.
pub fn mock_github(expectations: Vec<Expectation>) -> MockGitHub {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server address");
    let state = Arc::new(Mutex::new(ServerState {
        queue: expectations.into(),
        ..ServerState::default()
    }));

    let thread_state = Arc::clone(&state);
    std::thread::spawn(move || {
        // Connections are handled serially for fully deterministic
        // request ordering.
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            if handle_connection(stream, &thread_state).is_err() {
                break;
            }
        }
    });

    MockGitHub {
        base_url: format!("http://{addr}"),
        state,
    }
}

fn handle_connection(
    mut stream: std::net::TcpStream,
    state: &Arc<Mutex<ServerState>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end().to_string();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let content_length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let recorded = RecordedRequest {
        method: method.clone(),
        path: path.clone(),
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    };

    let response = {
        let mut state = state.lock();
        state.requests.push(recorded);
        match state.queue.pop_front() {
            Some(expectation)
                if expectation.method == method
                    && expectation.path == path =>
            {
                serialize_response(
                    expectation.status,
                    &expectation.headers,
                    &expectation.body,
                )
            }
            Some(expectation) => {
                state.mismatches.push(format!(
                    "expected {} {} but got {} {}",
                    expectation.method, expectation.path, method, path
                ));
                serialize_response(500, &[], "{\"message\":\"mock mismatch\"}")
            }
            None => {
                state
                    .mismatches
                    .push(format!("unexpected request: {method} {path}"));
                serialize_response(
                    500,
                    &[],
                    "{\"message\":\"mock had no expectation\"}",
                )
            }
        }
    };

    stream.write_all(&response)?;
    stream.flush()?;
    Ok(())
}

fn serialize_response(
    status: u16,
    headers: &[(String, String)],
    body: &str,
) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    ));
    let mut response = head.into_bytes();
    response.extend_from_slice(body.as_bytes());
    response
}

/// JSON for one label in GitHub's wire format.
pub fn label_json(
    name: &str,
    color: &str,
    description: Option<&str>,
) -> String {
    let description = match description {
        Some(text) => format!("\"{text}\""),
        None => "null".to_string(),
    };
    format!(
        "{{\"id\":1,\"node_id\":\"x\",\"url\":\"u\",\"name\":\
         \"{name}\",\"color\":\"{color}\",\"default\":false,\
         \"description\":{description},\"archived_at\":null,\
         \"archived_by\":null}}"
    )
}

/// A JSON array of [`label_json`] entries.
pub fn labels_json(labels: &[(&str, &str, Option<&str>)]) -> String {
    let entries: Vec<String> = labels
        .iter()
        .map(|(name, color, description)| {
            label_json(name, color, *description)
        })
        .collect();
    format!("[{}]", entries.join(","))
}

// ----- Binary-level test harness -----------------------------------------

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// Per-test isolation: a securely created throwaway configuration
/// directory plus a command builder that scrubbed every environment
/// variable labeldeck reads besides the ones a test sets explicitly.
///
/// The `tempfile::TempDir` owns the directory: it is created with an
/// unguessable name and restrictive permissions and removed when the
/// isolation is dropped.
pub struct Isolation {
    pub config_dir: PathBuf,
    /// Keeps the configuration directory alive until drop.
    _dir: tempfile::TempDir,
}

impl Isolation {
    pub fn new(tag: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("labeldeck-cli-{tag}-"))
            .tempdir()
            .expect("create isolated config directory");
        Self {
            config_dir: dir.path().to_path_buf(),
            _dir: dir,
        }
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_labeldeck"));
        command
            .args(args)
            .env("LABELDECK_CONFIG_DIR", &self.config_dir)
            .env_remove("LABELDECK_TOKEN")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("LABELDECK_API")
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy")
            .env_remove("HTTPS_PROXY")
            .env_remove("https_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");
        command
    }
}

/// Run a labeldeck command to completion, capturing both streams.
pub fn run(command: &mut Command) -> Output {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run labeldeck binary")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Point a labeldeck command at a mock GitHub API.
pub fn against_mock<'a>(
    mock: &MockGitHub,
    command: &'a mut Command,
) -> &'a mut Command {
    command.env("LABELDECK_API", mock.base_url())
}

/// Write a canonical label file inside the isolated config directory.
pub fn canonical_file(dir: &Isolation, name: &str, contents: &str) -> PathBuf {
    let path = dir.config_dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// Install a config.toml in the isolated config directory.
pub fn write_config(isolation: &Isolation, contents: &str) {
    std::fs::write(isolation.config_dir.join("config.toml"), contents)
        .unwrap();
}
