#[test]
fn cfg_miri_and_benign_code_pass() {
    assert!(cfg!(miri));
    let values = [2_u32, 3_u32];
    assert_eq!(values.iter().sum::<u32>(), 5);
}
