//! UI strings by the original's resource key (`Properties.Resources`), in the
//! user's UI language like `ResourceManager.GetString(key,
//! CultureInfo.CurrentUICulture)`.
//!
//! The tables are `locales/*.resx`, packed by build.rs into one embedded
//! blob. Only the active culture's entries are read, once, into an index of
//! slices of that blob. Lookups fall back like .NET resources: the exact
//! culture ("de-AT"), then its parents ("de"), then the neutral English
//! table; a key missing everywhere comes back as the key itself.
use std::sync::OnceLock;

mod tables {
    include!(concat!(env!("OUT_DIR"), "/i18n.rs"));
}

static BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/i18n.bin"));

pub use tables::KEYS;

struct Catalog {
    /// The UI culture's name as Windows reports it ("de-DE").
    name: String,
    rtl: bool,
    /// Text per key index.
    text: Box<[&'static str]>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Picks the UI culture: `culture` when given (`BT_CULTURE` for
/// `--render` and debug builds), otherwise the Windows display language.
/// Only the first call counts. Lookups before it use the neutral table,
/// which keeps tests independent of the machine's language.
pub fn init(culture: Option<&str>) {
    CATALOG.get_or_init(|| {
        let name = culture
            .map(str::to_owned)
            .or_else(windows_language)
            .unwrap_or_default();
        Catalog::load(&name)
    });
}

#[cfg(windows)]
fn windows_language() -> Option<String> {
    crate::win32::locale::ui_language()
}

#[cfg(not(windows))]
fn windows_language() -> Option<String> {
    None
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| Catalog::load(""))
}

impl Catalog {
    fn load(name: &str) -> Self {
        let mut text: Box<[&'static str]> = vec![""; KEYS.len()].into_boxed_slice();
        // Neutral first, then the most general parent down to the culture.
        for culture in chain(name).iter().rev() {
            if let Some(entries) = entries(culture) {
                for (id, value) in entries {
                    text[id] = value;
                }
            }
        }
        Self {
            name: name.to_owned(),
            rtl: is_rtl_culture(name),
            text,
        }
    }
}

/// The resource fallback chain for `name`, most specific first, ending with
/// the neutral culture "": "bs-Latn-BA" -> ["bs-latn-ba", "bs-latn", "bs", ""].
/// Like .NET, a sibling ("de-AT" for "de-DE") is not part of it.
fn chain(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = name.trim().replace('_', "-").to_ascii_lowercase();
    while !current.is_empty() {
        out.push(current.clone());
        match current.rfind('-') {
            Some(at) => current.truncate(at),
            None => current.clear(),
        }
    }
    out.push(String::new());
    out
}

/// A culture's entries from the blob, if the culture has a table.
fn entries(culture: &str) -> Option<impl Iterator<Item = (usize, &'static str)>> {
    let &(_, start, end) = tables::CULTURES.iter().find(|(c, _, _)| *c == culture)?;
    let mut rest = &BLOB[start as usize..end as usize];
    Some(std::iter::from_fn(move || {
        if rest.len() < 4 {
            return None;
        }
        let id = usize::from(u16::from_le_bytes([rest[0], rest[1]]));
        let len = usize::from(u16::from_le_bytes([rest[2], rest[3]]));
        let (bytes, tail) = rest[4..].split_at(len);
        rest = tail;
        // build.rs wrote these bytes from a String.
        Some((id, std::str::from_utf8(bytes).unwrap_or_default()))
    }))
}

/// Whether `name`'s writing system runs right to left, as .NET's
/// `TextInfo.IsRightToLeft`: an Arabic, Hebrew, Thaana, Syriac, N'Ko,
/// Adlam or Hanifi script subtag, else the language's default script.
pub fn is_rtl_culture(name: &str) -> bool {
    let lower = name.to_ascii_lowercase().replace('_', "-");
    let mut parts = lower.split('-');
    let language = parts.next().unwrap_or_default();
    if let Some(script) = parts.find(|p| p.len() == 4) {
        return matches!(
            script,
            "arab" | "hebr" | "thaa" | "syrc" | "nkoo" | "adlm" | "rohg" | "mand" | "samr"
        );
    }
    matches!(
        language,
        "ar" | "arc"
            | "ckb"
            | "dv"
            | "fa"
            | "he"
            | "iw"
            | "ks"
            | "ku"
            | "lrc"
            | "mzn"
            | "nqo"
            | "pnb"
            | "prs"
            | "ps"
            | "sd"
            | "syr"
            | "ug"
            | "ur"
            | "yi"
    )
}

fn id(key: &str) -> Option<usize> {
    KEYS.binary_search(&key).ok()
}

/// The text for `key` in the UI culture (`Resources.<key>`).
pub fn tr(key: &'static str) -> &'static str {
    match id(key) {
        Some(id) => catalog().text[id],
        None => {
            debug_assert!(false, "unknown string key {key}");
            key
        }
    }
}

/// `tr(key)` with placeholders replaced: each `{name}` whose name is in
/// `args` becomes its value, covering both the original's
/// `.Replace("{Name}", …)` keys and `string.Format` ones (`{0}`). Anything
/// else in braces is left as written.
pub fn trf(key: &'static str, args: &[(&str, &str)]) -> String {
    format(tr(key), args)
}

/// Replaces `{name}` placeholders in `text`; see `trf`.
pub fn format(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| {
            args.iter()
                .find(|(n, _)| *n == &after[..close])
                .map(|a| (close, a.1))
        }) {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The active UI culture's name ("" when Windows gave none).
pub fn culture() -> &'static str {
    &catalog().name
}

/// Whether the UI culture is right to left (`SystemSettings.IsRTL`):
/// windows lay out mirrored, like the original's
/// `FlowDirection="RightToLeft"`.
pub fn is_rtl() -> bool {
    catalog().rtl
}

/// Every culture with a table ("" is neutral), for tests.
#[cfg(test)]
fn culture_names() -> impl Iterator<Item = &'static str> {
    tables::CULTURES.iter().map(|(c, _, _)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_table_has_every_key_once_and_sorted() {
        assert!(KEYS.len() > 700);
        assert!(KEYS.windows(2).all(|w| w[0] < w[1]));
        let english = Catalog::load("");
        assert_eq!(english.text.len(), KEYS.len());
        assert_eq!(english.text[id("ContextMenuExitTitle").unwrap()], "Exit");
        assert_eq!(
            english.text[id("NoDevicesPanelContent").unwrap()],
            "It doesn't look like you have any playback devices."
        );
    }

    #[test]
    fn every_table_parses() {
        let mut count = 0;
        for culture in culture_names() {
            let entries: Vec<_> = entries(culture).unwrap().collect();
            assert!(!entries.is_empty(), "{culture}");
            assert!(entries.iter().all(|(id, _)| *id < KEYS.len()), "{culture}");
            count += 1;
        }
        // Neutral, the original's 33 cultures and nothing lost on the way.
        assert!(count >= 34, "{count}");
        // The last entry ends exactly at the blob's end.
        let (_, _, end) = tables::CULTURES.last().unwrap();
        assert_eq!(*end as usize, BLOB.len());
    }

    #[test]
    fn falls_back_like_resource_manager() {
        assert_eq!(
            chain("bs-Latn-BA"),
            ["bs-latn-ba", "bs-latn", "bs", ""].map(String::from)
        );
        let exit = id("ContextMenuExitTitle").unwrap();
        assert_eq!(Catalog::load("de-DE").text[exit], "Beenden");
        // No "de" table and siblings are not consulted: English.
        assert_eq!(Catalog::load("de-AT").text[exit], "Exit");
        // Case and separators do not matter.
        assert_eq!(Catalog::load("DE_de").text[exit], "Beenden");
        // A key the culture lacks comes from the neutral table.
        let ja = Catalog::load("ja-JP");
        let key = id("TrayStarProject").unwrap();
        assert_eq!(ja.text[key], Catalog::load("").text[key]);
        assert_ne!(ja.text[exit], "Exit");
    }

    #[test]
    fn decodes_entities_and_keeps_placeholders() {
        let fr = Catalog::load("fr-FR");
        let format = fr.text[id("AppOrDeviceFormatAccessibleText").unwrap()];
        assert!(
            format.contains("{Volume}") && format.contains("{Name}"),
            "{format}"
        );
        assert!(KEYS.iter().all(|k| !k.contains('&')));
        for culture in culture_names() {
            for (_, text) in entries(culture).unwrap() {
                assert!(
                    !text.contains("&amp;") && !text.contains("&lt;"),
                    "{culture}: {text}"
                );
                assert!(!text.contains('\r'), "{culture}: {text}");
            }
        }
    }

    #[test]
    fn formats_named_and_positional_placeholders() {
        assert_eq!(
            format(
                "{Volume}% volume - {Name}",
                &[("Name", "Edge"), ("Volume", "40")]
            ),
            "40% volume - Edge"
        );
        assert_eq!(
            format("Now using {0}", &[("0", "Speakers")]),
            "Now using Speakers"
        );
        assert_eq!(format("{unknown} {0", &[("0", "x")]), "{unknown} {0");
    }

    #[test]
    fn right_to_left_cultures() {
        for rtl in [
            "ar-SA",
            "he-IL",
            "fa-IR",
            "ur-PK",
            "ku-Arab-IQ",
            "ckb-IQ",
            "sd-Arab-PK",
        ] {
            assert!(is_rtl_culture(rtl), "{rtl}");
        }
        for ltr in [
            "en-US",
            "de-DE",
            "ja-JP",
            "",
            "uz-Latn-UZ",
            "pa-IN",
            "ku-Latn-TR",
        ] {
            assert!(!is_rtl_culture(ltr), "{ltr}");
        }
        assert!(is_rtl_culture("pa-Arab-PK"));
    }

    /// Every key the code asks for is in the neutral table: scans the
    /// sources for literal keys passed to `tr` and `trf`.
    #[test]
    fn every_key_used_in_code_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut used = Vec::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for call in ["tr(\"", "trf(\""] {
                        for (at, _) in text.match_indices(call) {
                            let before = text[..at].chars().last().unwrap_or(' ');
                            if before.is_alphanumeric() || before == '_' {
                                continue;
                            }
                            let rest = &text[at + call.len()..];
                            let key = &rest[..rest.find('"').unwrap()];
                            used.push((path.clone(), key.to_owned()));
                        }
                    }
                }
            }
        }
        assert!(used.len() > 50, "{}", used.len());
        let missing: Vec<_> = used
            .iter()
            .filter(|(_, key)| id(key).is_none())
            .map(|(path, key)| format!("{}: {key}", path.display()))
            .collect();
        assert!(missing.is_empty(), "keys not in locales/: {missing:#?}");
    }
}
