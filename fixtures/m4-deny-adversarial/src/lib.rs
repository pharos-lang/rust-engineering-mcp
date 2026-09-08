pub fn is_identifier_start(value: char) -> bool {
    unicode_ident::is_xid_start(value)
}
