pub fn pages_needed(items: u32, page_size: u32) -> Option<u32> {
    if page_size == 0 {
        None
    } else {
        Some((items + page_size - 1) / page_size)
    }
}
