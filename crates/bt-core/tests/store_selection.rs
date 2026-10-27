mod support;

use bt_core::{
    legacy_store::{PortableSnapshot, Read, StoreFlavor, StoreProbe},
    legacy_xml::{Kind, Value},
    newtonsoft::Token,
};

#[test]
fn markers_override_package_identity_in_every_probe_combination() {
    // StorageFactory.cs: both marker checks precede App.HasIdentity.
    for (marker, dot_marker, identity, expected) in [
        (false, false, false, StoreFlavor::Registry),
        (false, false, true, StoreFlavor::Package),
        (false, true, false, StoreFlavor::Portable),
        (false, true, true, StoreFlavor::Portable),
        (true, false, false, StoreFlavor::Portable),
        (true, false, true, StoreFlavor::Portable),
        (true, true, false, StoreFlavor::Portable),
        (true, true, true, StoreFlavor::Portable),
    ] {
        assert_eq!(
            StoreProbe {
                portable_marker_is_file: marker,
                dot_portable_is_file: dot_marker,
                has_package_identity: identity,
            }
            .select(),
            expected
        );
    }
}

#[test]
fn migration_retains_original_portable_bytes_and_unknown_values() {
    let source = b"\xef\xbb\xbf{/*future*/'Unknown.Addon':{'x':[1,null,true]},'Volume':1,'Volume':2,'volume':3}";
    let snapshot = PortableSnapshot::capture(Some(source.to_vec()), &support::Central);
    assert_eq!(snapshot.original_bytes(), Some(source.as_slice()));
    assert!(matches!(
        snapshot.store().raw("Unknown.Addon"),
        Some(Token::Object(_))
    ));
    assert_eq!(
        snapshot.store().read("Volume", Kind::I32),
        Read::Value(Value::I32(2))
    );
    assert_eq!(
        snapshot.store().read("volume", Kind::I32),
        Read::Value(Value::I32(3))
    );
    assert_eq!(snapshot.store().keys().count(), 3);
    // Parsing succeeds with Newtonsoft syntax; bytes still retain the BOM,
    // comments and duplicate entries that a normalized JSON export would lose.
    assert_eq!(snapshot.original_bytes(), Some(source.as_slice()));
}

#[test]
fn empty_missing_and_corrupt_inputs_remain_distinguishable_for_recovery() {
    for bytes in [None, Some(vec![]), Some(b"{broken\xff".to_vec())] {
        let snapshot = PortableSnapshot::capture(bytes.clone(), &support::Central);
        assert_eq!(snapshot.original_bytes(), bytes.as_deref());
        assert_eq!(snapshot.store().keys().count(), 0);
        assert_eq!(snapshot.store().read("Volume", Kind::I32), Read::Default);
    }
}
