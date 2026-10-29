/// A sorted copy of the input.
pub fn sorted(v: &[u32]) -> Vec<u32> {
    let mut out = v.to_vec();
    out.sort_unstable();
    out
}
