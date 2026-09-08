unsafe extern "C" {
    #[link_name = "m4_miri_deliberately_missing_foreign_function"]
    fn missing_foreign_function() -> i32;
}

#[test]
fn unknown_ffi_is_unsupported() {
    unsafe {
        std::hint::black_box(missing_foreign_function());
    }
}
