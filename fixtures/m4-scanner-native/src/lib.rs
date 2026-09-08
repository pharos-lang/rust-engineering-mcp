//! The scanner must ignore `unsafe`, `extern`, and `unsafe {}` in comments.

pub const KEYWORDS: &str = "unsafe extern unsafe { ignored }";

pub fn is_identifier_start(value: char) -> bool {
    unicode_ident::is_xid_start(value)
}
