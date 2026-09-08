#[test]
fn use_after_free_is_undefined_behavior() {
    let pointer = Box::into_raw(Box::new(42_u32));
    unsafe {
        drop(Box::from_raw(pointer));
        std::hint::black_box(*pointer);
    }
}
