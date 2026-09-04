pub fn repeat_first(values: &mut Vec<String>) -> Option<String> {
    let first = values.first().cloned()?;
    values.push(first.clone());
    Some(first)
}
