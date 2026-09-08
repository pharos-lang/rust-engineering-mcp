#[test]
#[allow(invalid_value)]
fn uninitialized_integer_is_undefined_behavior() {
    let value = unsafe { std::mem::MaybeUninit::<u32>::uninit().assume_init() };
    std::hint::black_box(value);
}
