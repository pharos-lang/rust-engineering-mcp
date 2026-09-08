#[test]
fn invalidated_shared_reference_is_undefined_behavior() {
    let mut value = 0_i32;
    let pointer = &raw mut value;
    let shared = unsafe { &*pointer };
    unsafe {
        *pointer = 1;
    }
    std::hint::black_box(shared);
}
