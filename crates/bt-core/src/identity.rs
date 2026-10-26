//! AppIdentity.cs matching with Windows filename semantics on every host.
//! Path helper adaptation attribution: rust/THIRD_PARTY_NOTICES.md.
use crate::text::{
    invariant_lower, is_blank, ordinal_contains, ordinal_ignore_case, ordinal_ignore_case_optional,
    ordinal_starts_with,
};

fn separator(c: u16) -> bool {
    c == b'/' as u16 || c == b'\\' as u16
}

/// Root shape rules follow .NET 8 PathInternal.Windows.GetRootLength; no disk
/// access or current-directory lookup is involved. The caller supplies UTF-16.
pub(crate) fn windows_root_length(path: &[u16]) -> usize {
    let n = path.len();
    let extended = n >= 4
        && path[0] == 92
        && (path[1] == 92 || path[1] == 63)
        && path[2] == 63
        && path[3] == 92;
    let device = extended
        || (n >= 4
            && separator(path[0])
            && separator(path[1])
            && (path[2] == 46 || path[2] == 63)
            && separator(path[3]));
    let device_unc = device && n >= 8 && path[4..7] == [85, 78, 67] && separator(path[7]);
    if (!device || device_unc) && n > 0 && separator(path[0]) {
        if device_unc || (n > 1 && separator(path[1])) {
            let mut index = if device_unc { 8 } else { 2 };
            let mut remaining = 2;
            while index < n {
                if separator(path[index]) {
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
                index += 1;
            }
            index
        } else {
            1
        }
    } else if device {
        let mut index = 4;
        while index < n && !separator(path[index]) {
            index += 1;
        }
        if index < n && index > 4 {
            index += 1;
        }
        index
    } else if n >= 2 && path[1] == 58 && path[0] <= 127 && (path[0] as u8).is_ascii_alphabetic() {
        if n > 2 && separator(path[2]) { 3 } else { 2 }
    } else {
        0
    }
}

pub(crate) fn filename_without_extension(path: &str) -> String {
    let path: Vec<u16> = path.encode_utf16().collect();
    let root = windows_root_length(&path);
    let start = path
        .iter()
        .rposition(|c| separator(*c))
        .map_or(root, |i| (i + 1).max(root));
    let name = &path[start..];
    let end = name.iter().rposition(|c| *c == 46).unwrap_or(name.len());
    String::from_utf16_lossy(&name[..end])
}

/// Windows Path.IsPathFullyQualified shape test; it does not validate the path
/// or consult the current directory. A single slash/root is still relative.
pub fn windows_is_fully_qualified(value: &str) -> bool {
    let path: Vec<u16> = value.encode_utf16().take(3).collect();
    if path.len() < 2 {
        return false;
    }
    if separator(path[0]) {
        return path[1] == 63 || separator(path[1]);
    }
    path.len() >= 3
        && path[1] == 58
        && separator(path[2])
        && path[0] <= 127
        && (path[0] as u8).is_ascii_alphabetic()
}

pub fn normalize_exe_name(typed: Option<&str>) -> String {
    let value = typed.unwrap_or_default().trim().trim_matches('"');
    if value.is_empty() {
        return String::new();
    }
    let name = filename_without_extension(&value.replace('\\', "/"));
    if is_blank(Some(&name)) {
        value.into()
    } else {
        name
    }
}

pub fn normalize_token(value: Option<&str>) -> String {
    if is_blank(value) {
        return String::new();
    }
    invariant_lower(&filename_without_extension(
        &value.unwrap_or_default().trim().replace('\\', "/"),
    ))
}

pub fn matches_exact(exe: Option<&str>, display: Option<&str>, query: Option<&str>) -> bool {
    if is_blank(query) {
        return false;
    }
    let query = query.unwrap_or_default();
    let exe = exe.unwrap_or_default();
    ordinal_ignore_case(exe, query)
        || ordinal_ignore_case(
            &normalize_exe_name(Some(exe)),
            &normalize_exe_name(Some(query)),
        )
        || ordinal_ignore_case_optional(display, Some(query))
}

pub fn matches_partial(exe: Option<&str>, display: Option<&str>, query: Option<&str>) -> bool {
    if is_blank(query) {
        return false;
    }
    let query = query.unwrap_or_default();
    ordinal_contains(exe.unwrap_or_default(), query)
        || ordinal_contains(display.unwrap_or_default(), query)
}

pub fn matches_app(exe: Option<&str>, display: Option<&str>, query: Option<&str>) -> bool {
    matches_exact(exe, display, query) || matches_partial(exe, display, query)
}

pub fn matches_device(display: Option<&str>, query: Option<&str>) -> bool {
    !is_blank(display)
        && !is_blank(query)
        && ordinal_contains(display.unwrap_or_default(), query.unwrap_or_default())
}

pub fn score(
    exe: Option<&str>,
    display: Option<&str>,
    app_id: Option<&str>,
    query: Option<&str>,
) -> i32 {
    let query = normalize_token(query);
    if query.trim().is_empty() {
        return 0;
    }
    [exe, Some(normalize_exe_name(exe).as_str()), display, app_id]
        .into_iter()
        .map(|candidate| {
            let token = normalize_token(candidate);
            if token.trim().is_empty() {
                0
            } else if ordinal_ignore_case(&token, &query) {
                100
            } else if ordinal_starts_with(&token, &query) {
                80
            } else if ordinal_contains(&token, &query) {
                60
            } else {
                0
            }
        })
        .max()
        .unwrap_or(0)
}
