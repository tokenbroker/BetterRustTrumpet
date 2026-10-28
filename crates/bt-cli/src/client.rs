//! PipeClient.SendCommand: connect (waiting up to the timeout while the pipe
//! does not exist or is busy), refuse a pipe owned by another user, write
//! the command and LF, read one line.
use crate::win32;
use bt_core::protocol;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::windows::fs::OpenOptionsExt,
    thread,
    time::{Duration, Instant},
};

/// ERROR_PIPE_BUSY: every instance is serving another client.
const PIPE_BUSY: i32 = 231;

/// This Windows session's pipe name.
pub fn pipe_name() -> String {
    protocol::rust_pipe_name(win32::session_id())
}

fn connect(name: &str, timeout: Duration) -> io::Result<File> {
    let path = format!(r"\\.\pipe\{name}");
    let deadline = Instant::now() + timeout;
    loop {
        // Identification only: the server cannot impersonate the caller.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(win32::SECURITY_IDENTIFICATION)
            .open(&path)
        {
            Ok(file) => return Ok(file),
            Err(e)
                if (e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(PIPE_BUSY))
                    && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Sends `command` to the pipe `name`. `None` when no server answers in
/// time or the pipe belongs to someone else (the original returns null).
pub fn send_to(name: &str, command: &str, timeout: Duration) -> Option<String> {
    let mut pipe = connect(name, timeout).ok()?;
    if !win32::owned_by_current_user(&pipe) {
        return None;
    }
    pipe.write_all(&protocol::encode_command(command)).ok()?;
    protocol::read_line_utf8(&mut pipe).ok()
}

/// Sends `command` to the running app with the original's 5 s timeout.
pub fn send(command: &str) -> Option<String> {
    let timeout = Duration::from_millis(protocol::CLIENT_CONNECT_TIMEOUT_MS.into());
    send_to(&pipe_name(), command, timeout)
}
