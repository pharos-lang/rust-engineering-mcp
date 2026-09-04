pub fn dangling() {
    let borrowed;
    {
        let owned = String::from("fixture");
        borrowed = &owned;
    }
    println!("{borrowed}");
}
