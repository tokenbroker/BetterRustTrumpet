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
