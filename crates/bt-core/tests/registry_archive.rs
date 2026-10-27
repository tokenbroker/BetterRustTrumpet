use bt_core::{
    migration,
    registry_snapshot::{RawRegistryValue, RegistrySnapshot},
};

#[test]
fn registry_archives_roundtrip_all_raw_units_and_unknown_types() {
    for snapshot in [
        RegistrySnapshot::default(),
        RegistrySnapshot {
            key_existed: true,
            values: vec![],
        },
        RegistrySnapshot {
            key_existed: true,
            values: vec![
                RawRegistryValue {
                    name_utf16: vec![],
                    kind: 1,
                    bytes: vec![97, 0, 98],
                },
                RawRegistryValue {
                    name_utf16: vec![0xd800, 0x65e5],
                    kind: u32::MAX,
                    bytes: (0..=255).collect(),
                },
                RawRegistryValue {
                    name_utf16: vec![0xdc00],
                    kind: 2,
                    bytes: vec![],
                },
                RawRegistryValue {
                    name_utf16: vec![65, 0, 66],
                    kind: 0,
                    bytes: vec![0],
                },
            ],
        },
    ] {
        let encoded = migration::encode_registry(&snapshot).unwrap();
        assert_eq!(migration::decode_registry(&encoded).unwrap(), snapshot);
    }
}

#[test]
fn invalid_snapshots_and_wrong_archive_flavors_fail() {
    let raw = RawRegistryValue {
        name_utf16: vec![b'a' as u16],
        kind: 0,
        bytes: vec![],
    };
    for snapshot in [
        RegistrySnapshot {
            key_existed: false,
            values: vec![raw.clone()],
        },
        RegistrySnapshot {
            key_existed: true,
            values: vec![raw.clone(), raw],
        },
    ] {
        assert!(migration::encode_registry(&snapshot).is_err());
    }
    assert!(migration::decode_registry(&migration::encode_portable(None).unwrap()).is_err());
    assert!(migration::decode_registry(br#"{"format":"BetterTrumpet.Rust.RegistryMigration","version":1,"snapshot":{"key_existed":true}}"#).is_err());
    assert!(migration::decode_registry(br#"{"format":"BetterTrumpet.Rust.RegistryMigration","version":99,"snapshot":{"key_existed":false,"values":[]}}"#).is_err());
}
