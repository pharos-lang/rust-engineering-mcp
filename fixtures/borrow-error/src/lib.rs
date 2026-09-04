pub fn conflict() {
    let mut values = vec![1];
    let first = &values[0];
    values.push(2);
    println!("{first}");
}
