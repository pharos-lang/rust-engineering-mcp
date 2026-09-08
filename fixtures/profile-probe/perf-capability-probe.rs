//! D24 feasibility probe: can an unprivileged, non-privileged-container guest
//! process open a user-space-only sampling perf event and map its ring buffer?
//! No crates, no libc: raw aarch64 syscalls only.
use std::arch::asm;

const SYS_PERF_EVENT_OPEN: u64 = 241;
const SYS_MMAP: u64 = 222;
const SYS_IOCTL: u64 = 29;
const SYS_CLOSE: u64 = 57;

unsafe fn syscall5(n: u64, a: u64, b: u64, c: u64, d: u64, e: u64) -> i64 {
    let ret: i64;
    unsafe {
        asm!("svc #0", in("x8") n, inlateout("x0") a as i64 => ret,
             in("x1") b, in("x2") c, in("x3") d, in("x4") e, in("x5") 0u64,
             options(nostack));
    }
    ret
}

fn main() {
    let mut attr = [0u8; 128];
    let put32 = |a: &mut [u8; 128], off: usize, v: u32| a[off..off + 4].copy_from_slice(&v.to_le_bytes());
    let put64 = |a: &mut [u8; 128], off: usize, v: u64| a[off..off + 8].copy_from_slice(&v.to_le_bytes());
    put32(&mut attr, 0, 1); // PERF_TYPE_SOFTWARE
    put32(&mut attr, 4, 128); // size
    put64(&mut attr, 8, 0); // PERF_COUNT_SW_CPU_CLOCK
    put64(&mut attr, 16, 99); // sample_freq
    put64(&mut attr, 24, 0x27); // IP|TID|TIME|CALLCHAIN
    // disabled|exclude_kernel|exclude_hv|mmap|comm|freq
    put64(&mut attr, 40, (1 << 0) | (1 << 5) | (1 << 6) | (1 << 8) | (1 << 9) | (1 << 10));

    let fd = unsafe { syscall5(SYS_PERF_EVENT_OPEN, attr.as_ptr() as u64, 0, (-1i64) as u64, (-1i64) as u64, 0) };
    println!("perf_event_open(user-space-only, self, any-cpu) -> {fd}");
    if fd < 0 {
        println!("RESULT=perf_event_open_denied errno={}", -fd);
        return;
    }
    let len: u64 = 9 * 4096;
    let map = unsafe { syscall5(SYS_MMAP, 0, len, 3 /*RW*/, 1 /*MAP_SHARED*/, fd as u64) };
    println!("mmap(ring buffer 8+1 pages) -> {map:#x}");
    let enable = unsafe { syscall5(SYS_IOCTL, fd as u64, 0x2400 /*PERF_EVENT_IOC_ENABLE*/, 0, 0, 0) };
    println!("ioctl(PERF_EVENT_IOC_ENABLE) -> {enable}");
    // Burn CPU so the sampler has something to record.
    let mut acc: u64 = 0;
    for i in 0..80_000_000u64 { acc = acc.wrapping_add(i ^ acc.rotate_left(7)); }
    let disable = unsafe { syscall5(SYS_IOCTL, fd as u64, 0x2401 /*DISABLE*/, 0, 0, 0) };
    println!("ioctl(PERF_EVENT_IOC_DISABLE) -> {disable} (acc={acc})");
    if map > 0 && (map as i64) > 0 {
        // data_head lives at offset 1024 of the metadata page.
        let head = unsafe { std::ptr::read_volatile((map as usize + 1024) as *const u64) };
        println!("ring buffer data_head after workload = {head}");
        println!("RESULT={}", if head > 0 { "samples_collected" } else { "no_samples" });
    } else {
        println!("RESULT=mmap_denied errno={}", -(map as i64));
    }
    unsafe { syscall5(SYS_CLOSE, fd as u64, 0, 0, 0, 0) };
}
