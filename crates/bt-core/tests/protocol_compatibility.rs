//! Framing expectations from Interop/Helpers/PipeClient.cs and PipeServer.cs.
use bt_core::protocol;
use std::io::Cursor;

#[test]
fn reads_one_utf8_message_ignoring_cr_without_overreading() {
    let mut stream = Cursor::new("vol\rume 音楽 50\r\nsecond\n".as_bytes());
    assert_eq!(
        protocol::read_line_utf8(&mut stream).unwrap(),
        "volume 音楽 50"
    );
    assert_eq!(protocol::read_line_utf8(&mut stream).unwrap(), "second");
    assert_eq!(protocol::read_line_utf8(&mut stream).unwrap(), "");
}

#[test]
fn eof_returns_partial_message_and_replaces_invalid_utf8() {
    let mut stream = Cursor::new(b"pi\xffng");
    assert_eq!(
        protocol::read_line_utf8(&mut stream).unwrap(),
        "pi\u{fffd}ng"
    );
}

#[test]
fn outgoing_frames_have_no_bom_and_server_strips_literal_newlines() {
    assert_eq!(
        protocol::encode_command("volume 音楽 50"),
        "volume 音楽 50\n".as_bytes()
    );
    assert_eq!(
        protocol::encode_response("{\r\n\"ok\":true\n}"),
        b"{\"ok\":true}\n"
    );
    assert_eq!(
        protocol::encode_response(r#"{"message":"a\nb"}"#),
        b"{\"message\":\"a\\nb\"}\n"
    );
    assert_eq!(protocol::pipe_name(12), "BetterTrumpet_IPC_v1_12");
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn rust_pipe_name_differs_from_the_csharp_app() {
    assert_eq!(protocol::rust_pipe_name(3), "BetterTrumpet-Rust_IPC_v1_3");
    assert_ne!(protocol::rust_pipe_name(3), protocol::pipe_name(3));
}

#[test]
fn entry_point_classification_matches_cli_entry_point() {
    use protocol::Invocation::*;
    assert_eq!(protocol::classify(&[]), None);
    assert_eq!(protocol::classify(&strings(&["-ServerName:x"])), None);
    assert_eq!(protocol::classify(&strings(&["--HELP"])), Help);
    assert_eq!(protocol::classify(&strings(&["/?"])), Help);
    assert_eq!(protocol::classify(&strings(&["-v"])), Version);
    assert_eq!(protocol::classify(&strings(&["--unknown"])), None);
    assert_eq!(protocol::classify(&strings(&["/unknown"])), None);
    assert_eq!(
        protocol::classify(&strings(&["--set-volume", "-5", "--app", "my app"])),
        Remote("set-volume -5 --app \"my app\"".into())
    );
    // A plain word is a preset alias; only one prefix of the line is removed.
    assert_eq!(
        protocol::classify(&strings(&["focus", ""])),
        Remote("focus ".into())
    );
    assert_eq!(
        protocol::classify(&strings(&["-ServerName:y", "//ping", "/x"])),
        Remote("/ping /x".into())
    );
    assert_eq!(
        protocol::classify(&strings(&["/ServerName:x", "ping"])),
        None
    );
    assert_eq!(protocol::remote_command(&strings(&["---x"])), "-x");
    assert_eq!(
        protocol::remote_command(&strings(&["a \"b\" c"])),
        "\"a \"b\" c\""
    );
}

#[test]
fn client_indents_like_newtonsoft_without_reordering() {
    assert_eq!(
        protocol::indent_json(r#"{"z":1,"a":[],"o":{},"s":"x,\"{[:","l":[{"k":true},null]}"#)
            .unwrap(),
        "{\r\n  \"z\": 1,\r\n  \"a\": [],\r\n  \"o\": {},\r\n  \"s\": \"x,\\\"{[:\",\r\n  \"l\": [\r\n    {\r\n      \"k\": true\r\n    },\r\n    null\r\n  ]\r\n}"
    );
    assert_eq!(protocol::indent_json("[ ]").unwrap(), "[]");
    assert_eq!(protocol::indent_json("not json"), None);
}
