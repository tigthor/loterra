pub fn count_match(x: &str, y: &str) -> usize {
    let mut count = 0;
    for i in 0..y.len() {
        if x.chars().nth(i).unwrap() == y.chars().nth(i).unwrap() {
            count += 1;
        }
    }
    count
}
