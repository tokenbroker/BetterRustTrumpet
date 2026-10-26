//! Portable .NET 8 invariant casing/slug behavior captured from the reference
//! runner. Keep profile IDs and application matching independent of host ICU.
mod data {
    include!("data/dotnet_text.rs");
}

fn mapped(code: u32, ranges: &[(u32, u32, i32)]) -> Option<char> {
    let index = ranges.partition_point(|(_, end, _)| *end < code);
    let &(start, _, delta) = ranges.get(index)?;
    if code < start {
        return None;
    }
    char::from_u32((i64::from(code) + i64::from(delta)) as u32)
}

pub fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|s| s.trim().is_empty())
}

pub fn invariant_lower(value: &str) -> String {
    value
        .chars()
        .map(|c| mapped(c as u32, data::INVARIANT_LOWER_RANGES).unwrap_or(c))
        .collect()
}

pub fn ordinal_ignore_case(left: &str, right: &str) -> bool {
    let fold = |c: char| mapped(c as u32, data::ORDINAL_UPPER_RANGES).unwrap_or(c);
    left.chars().map(fold).eq(right.chars().map(fold))
}

pub fn ordinal_ignore_case_optional(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => ordinal_ignore_case(left, right),
        (None, None) => true,
        _ => false,
    }
}

pub fn ordinal_starts_with(value: &str, prefix: &str) -> bool {
    let fold = |c: char| mapped(c as u32, data::ORDINAL_UPPER_RANGES).unwrap_or(c);
    let mut value = value.chars().map(fold);
    prefix.chars().map(fold).all(|c| value.next() == Some(c))
}

pub fn ordinal_contains(value: &str, query: &str) -> bool {
    let fold = |s: &str| {
        s.chars()
            .map(|c| mapped(c as u32, data::ORDINAL_UPPER_RANGES).unwrap_or(c))
            .collect::<String>()
    };
    fold(value).contains(&fold(query))
}

pub fn bmp_letter(c: char) -> bool {
    c.len_utf16() == 1 && mapped(c as u32, data::SLUG_RANGES).is_some() && !c.is_numeric()
}

/// VolumeProfileService.ToSlug iterates UTF-16 chars, so supplementary letters
/// are separators too. Generated ranges include only the source's accepted BMP
/// letters/decimal digits and their invariant lowercase output.
pub fn profile_slug(value: Option<&str>) -> String {
    let mut result = String::new();
    let mut separator = false;
    for c in value.unwrap_or_default().trim().chars() {
        let accepted = if c.len_utf16() == 1 {
            mapped(c as u32, data::SLUG_RANGES)
        } else {
            None
        };
        if let Some(c) = accepted {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.push(c);
            separator = false;
        } else {
            separator = true;
        }
    }
    result
}
