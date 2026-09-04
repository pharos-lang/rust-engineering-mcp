//! Container-only proc-macro fixture; the harness sets proc-macro=true.
extern crate proc_macro;
mod checks;

#[proc_macro]
pub fn verify_containment(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    assert!(input.is_empty(), "fixture takes no arguments");
    checks::run("proc_macro");
    eprintln!("RUST_CONTAINMENT_PROC_MACRO_CHECKS_PASSED");
    proc_macro::TokenStream::new()
}
