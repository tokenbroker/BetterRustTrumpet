//! The CLI pipe end to end with fake handlers: framing, the owner check,
//! empty commands, handler panics, timeouts and stop. No UI or audio.
#![cfg(windows)]
use bt_app::win32::pipe::PipeServer;
use bt_cli::client::send_to;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);

fn unique_name() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "BetterTrumpet-Rust-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
fn round_trips_one_line_per_connection() {
    let name = unique_name();
    let _server = PipeServer::start(&name, |command| {
        // Pretty JSON with line breaks: the server sends it on one line.
        format!(
            "{{\r\n\"got\": {}\n}}",
            serde_json::to_string(command).unwrap()
        )
    })
    .unwrap();
    for command in ["ping", "volume 音楽 50", "set-volume -5 --app \"my app\""] {
        let response = send_to(&name, command, TIMEOUT).unwrap();
        let expected = format!("{{\"got\": {}}}", serde_json::to_string(command).unwrap());
        assert_eq!(response, expected);
    }
}

#[test]
fn ignores_cr_in_commands_and_skips_empty_ones() {
    let name = unique_name();
    let _server = PipeServer::start(&name, |command| format!("\"{command}\"")).unwrap();
    assert_eq!(send_to(&name, "pi\rng", TIMEOUT).unwrap(), "\"ping\"");
    // An empty command gets no answer: the server just disconnects.
    let path = format!(r"\\.\pipe\{name}");
    // Between connections the pipe briefly does not exist; clients retry.
    let started = Instant::now();
    let mut pipe = loop {
        match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(pipe) => break pipe,
            Err(_) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    };
    pipe.write_all(b"\r\n").unwrap();
    let mut rest = Vec::new();
    let _ = pipe.read_to_end(&mut rest);
    assert!(rest.is_empty());
    // The server keeps serving afterwards.
    assert_eq!(send_to(&name, "after", TIMEOUT).unwrap(), "\"after\"");
}

#[test]
fn a_panicking_handler_answers_an_error() {
    let name = unique_name();
    let _server = PipeServer::start(&name, |command| {
        if command == "boom" {
            panic!("it \"broke\"");
        }
        "{}".into()
    })
    .unwrap();
    assert_eq!(
        send_to(&name, "boom", TIMEOUT).unwrap(),
        r#"{"error":"it \"broke\""}"#
    );
    assert_eq!(send_to(&name, "fine", TIMEOUT).unwrap(), "{}");
}

#[test]
fn missing_server_times_out_and_stop_closes_the_pipe() {
    let name = unique_name();
    let started = Instant::now();
    assert_eq!(send_to(&name, "ping", Duration::from_millis(200)), None);
    assert!(started.elapsed() >= Duration::from_millis(200));

    let mut server = PipeServer::start(&name, |_| "{}".into()).unwrap();
    assert_eq!(send_to(&name, "ping", TIMEOUT).as_deref(), Some("{}"));
    server.stop();
    assert_eq!(send_to(&name, "ping", Duration::from_millis(200)), None);
}

#[test]
fn session_pipe_name_is_the_rust_one() {
    let name = bt_app::win32::pipe::session_pipe_name();
    assert!(name.starts_with("BetterTrumpet-Rust_IPC_v1_"), "{name}");
    assert_eq!(name, bt_cli::client::pipe_name());
}
