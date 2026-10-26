//! UTF-8 line framing from Interop/Helpers/PipeClient.cs and PipeServer.cs.
use std::io::{self, Read};

pub const CLIENT_CONNECT_TIMEOUT_MS: u32 = 5_000;
pub const PING_TIMEOUT_MS: u32 = 1_000;
pub const STOP_CONNECT_TIMEOUT_MS: u32 = 500;

pub fn pipe_name(windows_session_id: u32) -> String {
    format!("BetterTrumpet_IPC_v1_{windows_session_id}")
}

/// Read one message without consuming bytes after its LF. CR is ignored even
/// inside a message, EOF terminates a partial line, invalid UTF-8 is replaced.
pub fn read_line_utf8(reader: &mut impl Read) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) if byte[0] == b'\r' => {}
            Ok(_) => bytes.push(byte[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn encode_command(command: &str) -> Vec<u8> {
    format!("{command}\n").into_bytes()
}

pub fn encode_response(response: &str) -> Vec<u8> {
    let mut bytes: Vec<_> = response
        .bytes()
        .filter(|b| !matches!(b, b'\r' | b'\n'))
        .collect();
    bytes.push(b'\n');
    bytes
}
