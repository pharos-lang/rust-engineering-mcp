pub fn owned_label(input: &str) -> String {
    let borrowed;
    {
        let temporary = input.trim().to_owned();
        borrowed = temporary.as_str();
    }
    borrowed.to_owned()
}
