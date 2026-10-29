/// How many of the values are even.
pub fn count_evens(v: &[u32]) -> usize {
    v.iter().filter(|x| *x % 2 == 0).count()
}
