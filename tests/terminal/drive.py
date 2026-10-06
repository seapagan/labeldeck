"""Drive a real Unix terminal by observed output, without timing-based key sends."""
import errno
import fcntl
import json
import os
import pty
import select
import signal
import struct
import sys
import termios
import time

steps = json.loads(sys.argv[2])
child, terminal = pty.fork()
if child == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
    os.execvpe(sys.argv[1], sys.argv[3:], os.environ)

reaped = False
transcript = bytearray()
pending = bytearray()
deadline = time.monotonic() + 20
try:
    while time.monotonic() < deadline:
        readable, _, _ = select.select([terminal], [], [], 0.1)
        if readable:
            try:
                chunk = os.read(terminal, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            transcript.extend(chunk)
            pending.extend(chunk)
            if b"\x1b[6n" in chunk:
                os.write(terminal, b"\x1b[1;1R")
            while steps and steps[0][0].encode() in pending:
                _, keys = steps.pop(0)
                pending.clear()
                os.write(terminal, keys.encode())
    else:
        raise RuntimeError("terminal driver timed out")
    _, status = os.waitpid(child, 0)
    reaped = True
    if steps:
        raise RuntimeError(f"unreached terminal step: {steps[0][0]}")
    if os.waitstatus_to_exitcode(status) != 0:
        raise RuntimeError(f"child exit status {os.waitstatus_to_exitcode(status)}")
    if b"\x1b[?1049l" not in transcript:
        raise RuntimeError("alternate screen was not restored")
    sys.stdout.buffer.write(transcript)
except Exception:
    sys.stdout.buffer.write(transcript)
    raise
finally:
    os.close(terminal)
    if not reaped:
        try:
            os.kill(child, signal.SIGKILL)
            os.waitpid(child, 0)
        except ProcessLookupError:
            pass
