//! What the adapter knows about proptest: the environment variables that
//! make a run reproducible from the seed alone, and the shape of its failure
//! message, from which the witness is read.
pub const RNG_SEED: &str = "PROPTEST_RNG_SEED";
pub const CASES: &str = "PROPTEST_CASES";
pub const DISABLE_FAILURE_PERSISTENCE: &str = "PROPTEST_DISABLE_FAILURE_PERSISTENCE";

const INPUT_MARK: &str = "minimal failing input:";
const WHY_MARK: &str = "Test failed:";

/// The witness text for a failed case: for a proptest failure, the shrunk
/// input and the reason (`x = 1, (lo, hi) = (0, 0); clamp_to(1, 0, 0) = 1`);
/// for any other failing test, the panic message without its location line
/// and backtrace note.
pub fn witness_text(message: &str, body: &str) -> String {
    match body.find(INPUT_MARK) {
        Some(at) => {
            let input = collapse(until_stats(&body[at + INPUT_MARK.len()..]));
            match why(body) {
                Some(why) => format!("{input}; {why}"),
                None => input,
            }
        }
        None => panic_text(message, body),
    }
}

fn until_stats(text: &str) -> &str {
    let end = text
        .lines()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .find(|(_, line)| line.trim_start().starts_with("successes:"))
        .map_or(text.len(), |(start, _)| start);
    &text[..end]
}

/// Lines joined, with the spacing a pretty-printed `{:#?}` adds removed.
fn collapse(text: &str) -> String {
    let joined: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    joined
        .join(" ")
        .replace("( ", "(")
        .replace(", )", ")")
        .replace(" )", ")")
        .replace("[ ", "[")
        .replace(", ]", "]")
        .replace(" ]", "]")
}

fn why(body: &str) -> Option<String> {
    let line = body.lines().find(|l| l.trim_start().starts_with(WHY_MARK))?;
    Some(line.trim_start()[WHY_MARK.len()..].trim().trim_end_matches('.').to_string())
}

fn panic_text(message: &str, body: &str) -> String {
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("thread '") && !l.starts_with("note: run with"))
        .collect();
    if lines.is_empty() { message.trim().to_string() } else { lines.join("; ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proptest_failure_yields_input_and_reason() {
        let body = "thread 'clamp' panicked at tests/props.rs:11:1:\nTest failed: clamp_to(1, 0, 0) = 1 at tests/props.rs:22.\nminimal failing input: x = 1, (lo, hi) = (\n    0,\n    0,\n)\nsuccesses: 5\nlocal rejects: 0\n";
        assert_eq!(
            witness_text("ignored", body),
            "x = 1, (lo, hi) = (0, 0); clamp_to(1, 0, 0) = 1 at tests/props.rs:22"
        );
    }

    #[test]
    fn plain_panic_yields_its_message() {
        let body = "thread 'exact' panicked at tests/u.rs:7:5:\nassertion `left == right` failed\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1`\n";
        assert_eq!(
            witness_text("thread 'exact' panicked", body),
            "assertion `left == right` failed; left: 1; right: 2"
        );
        assert_eq!(witness_text("short", ""), "short");
    }
}
