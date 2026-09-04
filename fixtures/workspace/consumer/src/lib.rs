pub fn answer() -> u32 { fixture_workspace_core::answer() }
#[test]
fn path_dependency() { assert_eq!(answer(), 42); }
