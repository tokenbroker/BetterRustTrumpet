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

/// The Rust port's pipe. The C# app keeps [`pipe_name`] so both can run side
/// by side, like the single-instance mutex (`Local\BetterTrumpet-Rust-…`).
pub fn rust_pipe_name(windows_session_id: u32) -> String {
    format!("BetterTrumpet-Rust_IPC_v1_{windows_session_id}")
}

/// What CliEntryPoint.TryHandleCliArgs does with the process arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    Help,
    Version,
    /// Send this command line to the running instance.
    Remote(String),
    /// Not a CLI invocation (the original starts the app normally).
    None,
}

const REMOTE_COMMANDS: [&str; 35] = [
    "list-devices",
    "list-apps",
    "resolve-apps",
    "get-volume",
    "set-volume",
    "mute",
    "unmute",
    "toggle-mute",
    "get-default",
    "set-default",
    "set-device",
    "list-profiles",
    "apply-profile",
    "presets",
    "save",
    "apply",
    "delete",
    "rule-preview",
    "rule-apply",
    "preset-create",
    "watch",
    "check-update",
    "export-settings",
    "import-settings",
    "ping",
    "doctor",
    "batch",
    "volume",
    "mode",
    "profile",
    "preset",
    "device",
    "apps",
    "update",
    "settings",
];

/// CliEntryPoint.TryHandleCliArgs: WPF `-ServerName:` arguments are
/// dropped; a known command (any `-`/`/` prefix) or a plain word (a preset
/// alias) goes to the running instance.
pub fn classify(args: &[String]) -> Invocation {
    let args: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with("-ServerName:"))
        .cloned()
        .collect();
    let Some(first) = args.first() else {
        return Invocation::None;
    };
    let command = crate::text::invariant_lower(first.trim_start_matches(['-', '/']));
    match command.as_str() {
        "help" | "h" | "?" => Invocation::Help,
        "version" | "v" => Invocation::Version,
        c if REMOTE_COMMANDS.contains(&c) => Invocation::Remote(remote_command(&args)),
        _ if !first.starts_with(['-', '/']) => Invocation::Remote(remote_command(&args)),
        _ => Invocation::None,
    }
}

/// CliEntryPoint.HandleRemoteCommand: arguments with a space are quoted
/// (embedded quotes are not escaped) and one leading `--`, `-` or `/` of the
/// whole line is removed.
pub fn remote_command(args: &[String]) -> String {
    let command = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(rest) = command.strip_prefix("--") {
        rest.to_owned()
    } else if let Some(rest) = command.strip_prefix(['-', '/']) {
        rest.to_owned()
    } else {
        command
    }
}

/// The client's presentation: `JToken.Parse(response).ToString(Indented)`,
/// two-space indents and CRLF line breaks, or `None` when the response is not
/// JSON (it is then printed as received). Property order and the original
/// token text are kept.
pub fn indent_json(text: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).ok()?;
    const NEWLINE: &str = "\r\n";
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let indent = |out: &mut String, depth: usize| {
        out.push_str(NEWLINE);
        out.extend(std::iter::repeat_n(' ', depth * 2));
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(c) = chars.next() {
                    out.push(c);
                    match c {
                        '\\' =>out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '{' | '[' => {
                while chars.next_if(|c| c.is_ascii_whitespace()).is_some() {}
                let close = if c == '{' { '}' } else { ']' };
                if chars.next_if_eq(&close).is_some() {
                    out.push(c);
                    out.push(close);
                } else {
                    out.push(c);
                    depth += 1;
                    indent(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                indent(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                indent(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_ascii_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}
