pub fn covered(value: bool) -> u8 {
    if value { 1 } else { 2 }
}

#[cfg(test)]
mod tests { #[test] fn one_arm_is_covered() { assert_eq!(super::covered(true), 1); } }
