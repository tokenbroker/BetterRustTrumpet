//! .NET 8 NumberStyles.Integer rules, with culture signs supplied by the edge.
//! Adaptation sources/license are recorded in rust/THIRD_PARTY_NOTICES.md.
#[derive(Clone, Copy, Debug)]
pub struct IntegerFormat<'a> {
    pub positive_sign: &'a str,
    pub negative_sign: &'a str,
}
impl IntegerFormat<'_> {
    pub const INVARIANT: Self = Self {
        positive_sign: "+",
        negative_sign: "-",
    };

    fn allow_hyphen(self) -> bool {
        matches!(
            self.negative_sign,
            "\u{2012}"
                | "\u{207b}"
                | "\u{208b}"
                | "\u{2212}"
                | "\u{2796}"
                | "\u{fe63}"
                | "\u{ff0d}"
        )
    }
    pub fn parse_i32(self, value: &str) -> Option<i32> {
        let mut value = value.trim_start_matches(integer_space);
        let mut negative = false;
        if self.positive_sign == "+" && self.negative_sign == "-" {
            if let Some(rest) = value.strip_prefix('-') {
                negative = true;
                value = rest;
            } else if let Some(rest) = value.strip_prefix('+') {
                value = rest;
            }
        } else if self.allow_hyphen() && value.starts_with('-') {
            negative = true;
            value = &value[1..];
        } else if !self.positive_sign.is_empty() && value.starts_with(self.positive_sign) {
            value = &value[self.positive_sign.len()..];
        } else if !self.negative_sign.is_empty() && value.starts_with(self.negative_sign) {
            negative = true;
            value = &value[self.negative_sign.len()..];
        }
        let digits = value.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let limit = if negative { 2147483648_u32 } else { 2147483647 };
        let mut magnitude = 0_u32;
        for digit in value[..digits].bytes() {
            magnitude = magnitude
                .checked_mul(10)?
                .checked_add(u32::from(digit - b'0'))?;
            if magnitude > limit {
                return None;
            }
        }
        let trailing = value[digits..].trim_start_matches(integer_space);
        if !trailing.bytes().all(|b| b == 0) {
            return None;
        }
        Some(if negative {
            (-i64::from(magnitude)) as i32
        } else {
            magnitude as i32
        })
    }
}
fn integer_space(c: char) -> bool {
    c == ' ' || ('\t'..='\r').contains(&c)
}
