use std::cell::UnsafeCell;
use std::sync::{Arc, Barrier};

struct Shared(UnsafeCell<u32>);

unsafe impl Sync for Shared {}

static VALUE: Shared = Shared(UnsafeCell::new(0));

#[test]
fn unordered_thread_writes_are_a_data_race() {
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();
    for value in [1_u32, 2_u32] {
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            unsafe {
                *VALUE.0.get() = value;
            }
        }));
    }
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
}
