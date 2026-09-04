#![deny(unsafe_code)]
pub fn read(value: &u8) -> u8 {
    unsafe { *(value as *const u8) }
}
