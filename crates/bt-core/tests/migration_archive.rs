mod support;
use bt_core::{legacy_store::PortableLoadStatus, migration};
use serde_json::json;

#[test]
fn archive_preserves_missing_empty_unknown_and_corrupt_source_bytes() {
    for (bytes, status) in [
        (None, PortableLoadStatus::Missing),
        (Some(vec![]), PortableLoadStatus::EmptyDocument),
        (Some(b"null".to_vec()), PortableLoadStatus::EmptyDocument),
        (
            Some(b"{/*x*/Unknown:{a:[1,undefined]},Volume:1,Volume:2}".to_vec()),
            PortableLoadStatus::Loaded,
        ),
        (Some(b"\xff\xfe{\0}\0".to_vec()), PortableLoadStatus::Loaded),
        (Some(b"{bad\xff".to_vec()), PortableLoadStatus::Invalid),
    ] {
        let encoded = migration::encode_portable(bytes.as_deref()).unwrap();
        let decoded = migration::decode_portable(&encoded, &support::Central).unwrap();
        assert_eq!(decoded.original_bytes(), bytes.as_deref());
        assert_eq!(decoded.status(), status);
    }
}

#[test]
fn missing_payload_corruption_and_unknown_versions_are_rejected() {
    for input in [
        json!({"format":migration::PORTABLE_FORMAT,"version":1}),
        json!({"format":migration::PORTABLE_FORMAT,"version":2,"original_bytes":null}),
        json!({"format":"other","version":1,"original_bytes":null}),
        json!({"format":migration::PORTABLE_FORMAT,"version":1,"original_bytes":[256]}),
        json!({"format":migration::PORTABLE_FORMAT,"version":1,"original_bytes":[-1]}),
    ] {
        assert!(
            migration::decode_portable(&serde_json::to_vec(&input).unwrap(), &support::Central)
                .is_err(),
            "{input}"
        );
    }
    assert!(migration::decode_portable(b"{truncated", &support::Central).is_err());
}
