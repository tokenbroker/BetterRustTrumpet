mod support;
use bt_core::{
    legacy_store::PortableSnapshot,
    legacy_xml::{Kind, Value},
    overrides::{EncodedValue, Overrides},
    registry_snapshot::{RawRegistryValue, RegistrySnapshot},
    registry_values::{Environment, ExpansionError, Read, Request},
};

fn text(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
struct Exact;
impl Environment for Exact {
    fn names_equal(&self, a: &[u16], b: &[u16]) -> bool {
        a == b
    }
    fn expand(&self, text: &[u16]) -> Result<Vec<u16>, ExpansionError> {
        Ok(text.to_vec())
    }
}

#[test]
fn unknown_fragments_survive_known_value_updates_without_numeric_rounding() {
    let source=br#"{"format":"BetterTrumpet.Rust.Settings","version":1,"future":1.2345678901234567890123456789,"entries":[{"key":[107],"note":{"x":1e9999},"value":{"encoding":"legacy-string","data":[111,108,100],"future":999999999999999999999999999999}},{"key":[117],"value":{"encoding":"future","data":{"opaque":1e99999}}}]}"#;
    let mut overrides = Overrides::decode(source).unwrap();
    overrides
        .set(
            &text("k"),
            EncodedValue::Text(vec![0xd800, 0, 0xdc00]),
            |a, b| a == b,
        )
        .unwrap();
    let encoded = String::from_utf8(overrides.encode().unwrap()).unwrap();
    for raw in [
        "1.2345678901234567890123456789",
        "1e9999",
        "999999999999999999999999999999",
        "1e99999",
    ] {
        assert!(encoded.contains(raw), "lost fragment {raw}");
    }
    let reloaded = Overrides::decode(encoded.as_bytes()).unwrap();
    assert_eq!(
        reloaded.get(&text("k"), |a, b| a == b).unwrap(),
        Some(EncodedValue::Text(vec![0xd800, 0, 0xdc00]))
    );
    assert!(reloaded.get(&text("u"), |a, b| a == b).is_err());
    let before = reloaded.encode().unwrap();
    let mut failed = reloaded;
    assert!(
        failed
            .set(&text("u"), EncodedValue::NativeBool(false), |a, b| a == b)
            .is_err()
    );
    assert_eq!(failed.encode().unwrap(), before);
}

#[test]
fn overlay_reads_shadow_and_reveal_immutable_archives_with_legacy_string_routes() {
    let original = b"{Volume:42,Unknown:{x:1},Empty:null}";
    let portable = PortableSnapshot::capture(Some(original.to_vec()), &support::Central);
    let registry = RegistrySnapshot {
        key_existed: true,
        values: vec![RawRegistryValue {
            name_utf16: text("Volume"),
            kind: 1,
            bytes: "<int>42</int>\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
        }],
    };
    let mut overrides = Overrides::new();
    assert_eq!(
        overrides
            .portable_read(&portable, "Volume", Request::Value(Kind::I32), |a, b| a
                == b)
            .unwrap(),
        Read::Value(Value::I32(42))
    );
    assert_eq!(
        overrides
            .registry_read(&registry, Some("Volume"), Request::Value(Kind::I32), &Exact)
            .unwrap(),
        Read::Value(Value::I32(42))
    );
    overrides
        .set(
            &text("Volume"),
            EncodedValue::Text(text("<int>17</int>")),
            |a, b| a == b,
        )
        .unwrap();
    assert_eq!(
        overrides
            .registry_read(&registry, Some("Volume"), Request::Value(Kind::I32), &Exact)
            .unwrap(),
        Read::Value(Value::I32(17))
    );
    assert_eq!(
        overrides
            .portable_read(&portable, "Volume", Request::Value(Kind::I32), |a, b| a
                == b)
            .unwrap(),
        Read::Value(Value::I32(17))
    );
    let nil = "<string xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:nil=\"true\"/>";
    overrides
        .set(&text("s"), EncodedValue::Text(text(nil)), |a, b| a == b)
        .unwrap();
    assert_eq!(
        overrides
            .registry_read(&registry, Some("s"), Request::NullDefaultString, &Exact)
            .unwrap(),
        Read::Value(Value::String(None))
    );
    assert_eq!(
        overrides
            .portable_read(&portable, "s", Request::NullDefaultString, |a, b| a == b)
            .unwrap(),
        Read::StringUtf16(text(nil))
    );
    assert!(overrides.portable_has_key(&portable, "Empty", |a, b| a == b));
    overrides.remove_override(&text("Volume"), |a, b| a == b);
    assert_eq!(
        overrides
            .registry_read(&registry, Some("Volume"), Request::Value(Kind::I32), &Exact)
            .unwrap(),
        Read::Value(Value::I32(42))
    );
    assert_eq!(portable.original_bytes(), Some(original.as_slice()));
}

#[test]
fn versions_missing_fields_duplicate_keys_and_payload_errors_are_explicit() {
    for source in [
        r#"{"format":"future","version":1,"entries":[]}"#,
        r#"{"format":"BetterTrumpet.Rust.Settings","version":2,"entries":[]}"#,
        r#"{"format":"BetterTrumpet.Rust.Settings","version":1}"#,
        r#"{"format":"BetterTrumpet.Rust.Settings","version":1,"entries":[{"key":[1],"value":{}},{"key":[1],"value":{}}]}"#,
    ] {
        assert!(Overrides::decode(source.as_bytes()).is_err());
    }
    let mut overrides = Overrides::new();
    overrides
        .set(&text("bool"), EncodedValue::NativeBool(false), |a, b| {
            a == b
        })
        .unwrap();
    assert_eq!(
        Overrides::decode(&overrides.encode().unwrap())
            .unwrap()
            .get(&text("bool"), |a, b| a == b)
            .unwrap(),
        Some(EncodedValue::NativeBool(false))
    );
    assert!(
        overrides
            .registry_read(
                &RegistrySnapshot::default(),
                Some("bool"),
                Request::Value(Kind::Bool),
                &Exact
            )
            .is_err()
    );
}
