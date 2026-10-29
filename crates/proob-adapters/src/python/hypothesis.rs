//! What the adapter knows about Hypothesis: the settings profile proob
//! registers through a plugin module it writes per run, the pytest options
//! that load the profile and fix the seed, the environment that keeps
//! `.hypothesis/` out of the tree, and the shape of a failure's text, from
//! which the witness is read.
use crate::Budget;

pub const PROFILE: &str = "proob";
pub const PLUGIN_MODULE: &str = "proob_hypothesis";
pub const PLUGIN_FILE: &str = "proob_hypothesis.py";
pub const PROFILE_OPTION: &str = "--hypothesis-profile";
pub const SEED_OPTION: &str = "--hypothesis-seed";
/// Where Hypothesis keeps its example database and failure patches; pointed
/// at a temporary directory so a run writes nothing into the repository.
pub const STORAGE_DIR_ENV: &str = "HYPOTHESIS_STORAGE_DIRECTORY";
pub const NO_BYTECODE_ENV: &str = "PYTHONDONTWRITEBYTECODE";

/// The plugin module: registers the `proob` profile that
/// `--hypothesis-profile proob` loads. No example database, so the seed alone
/// replays a run (the analogue of `PROPTEST_DISABLE_FAILURE_PERSISTENCE`); no
/// deadline, so wall-clock noise never fails a test; `print_blob`, so the
/// failure text carries the reproduction decorator.
pub fn plugin_text(budget: Budget) -> String {
    format!(
        "\"\"\"Written by proob for one run; never read from the repository.\"\"\"\n\
         from hypothesis import settings\n\
         \n\
         settings.register_profile(\n\
         \x20   \"{PROFILE}\",\n\
         \x20   max_examples={},\n\
         \x20   database=None,\n\
         \x20   deadline=None,\n\
         \x20   derandomize=False,\n\
         \x20   print_blob=True,\n\
         )\n",
        budget.0
    )
}

/// The lines Hypothesis puts in front of the shrunk input, by version
/// (6.169 says `Failing test case:`; earlier releases `Falsifying example:`).
const INPUT_MARKS: [&str; 3] = ["Failing test case:", "Falsifying example:", "Falsifying explicit example:"];

/// The witness text for a failed case: for a Hypothesis failure, the shrunk
/// arguments and the failure's first line (`takes=[6]; AssertionError: took 6
/// with 5 left`); for any other failing test, that first line alone. Only
/// those two parts are taken because pytest's assertion introspection prints
/// object addresses (`<TokenBucket object at 0x…>`), which would make two
/// runs of one seed disagree.
pub fn witness_text(message: &str, body: &str) -> String {
    let reason = reason(message, body);
    let input = input_block(&clean_lines(message)).or_else(|| input_block(&clean_lines(body)));
    match input {
        Some(input) if reason.is_empty() => input,
        Some(input) => format!("{input}; {reason}"),
        None => reason,
    }
}

/// The failure's first line: the report's message, else the first `E ` line
/// of the traceback (the body's other lines are source context), else its
/// first line of anything.
fn reason(message: &str, body: &str) -> String {
    first_line(message)
        .or_else(|| body.lines().map(str::trim).find_map(|l| l.strip_prefix("E ")).map(|l| l.trim().to_string()))
        .or_else(|| first_line(body))
        .unwrap_or_default()
}

/// Lines with pytest's `E ` failure prefix removed and trimmed.
fn clean_lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.trim().strip_prefix("E ").map_or_else(|| l.trim(), str::trim).to_string()).collect()
}

fn first_line(text: &str) -> Option<String> {
    clean_lines(text).into_iter().find(|l| !l.is_empty())
}

/// The arguments of the `test_x(\n    a=1,\n    b=2,\n)` block after a mark.
fn input_block(lines: &[String]) -> Option<String> {
    let at = lines.iter().position(|l| INPUT_MARKS.iter().any(|m| l.starts_with(m)))?;
    let head = &lines[at];
    if let Some(inline) = head.find('(').filter(|_| head.ends_with(')')).map(|i| &head[i + 1..head.len() - 1]) {
        return Some(if inline.is_empty() {
            head.rsplit(':').next().unwrap_or(head).trim().to_string()
        } else {
            inline.to_string()
        });
    }
    let end = lines[at + 1..].iter().position(|l| l == ")").map(|i| at + 1 + i)?;
    let args: Vec<&str> = lines[at + 1..end].iter().filter_map(|l| uncommented(l)).collect();
    let joined = args.join(" ");
    let joined = joined.trim_end_matches(',');
    Some(if joined.is_empty() {
        head.rsplit(':').next().unwrap_or(head).trim().to_string()
    } else {
        joined.to_string()
    })
}

/// An argument line without Hypothesis's explain-phase comments: a line that
/// is only a comment (`# The test always failed when …`) is dropped, and a
/// trailing `  # or any other generated value` is cut. Hypothesis separates
/// those comments with two spaces, which a `#` inside a value's repr never has.
fn uncommented(line: &str) -> Option<&str> {
    let line = line.split("  # ").next().unwrap_or(line).trim();
    (!line.is_empty() && !line.starts_with('#')).then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGE: &str = "AssertionError: took 6 with 5 left\nassert -1 >= 0\n +  where -1 = <interval.bucket.TokenBucket object at 0x10bc13ad0>.tokens\nFailing test case: test_bucket_never_goes_negative(\n    takes=[6],\n)\n\nYou can reproduce this test case by temporarily adding @reproduce_failure('6.169.4', b'AAFBBgA=') as a decorator on your test function";

    #[test]
    fn explain_phase_comments_are_not_part_of_the_witness() {
        let message = "AssertionError: x\nFailing test case: test_bump(\n    # The test always failed when commented parts were varied together.\n    version=Version(\n        0,  # or any other generated value\n        1,\n    ),\n    token='a#b',  # or any other generated value\n)\n";
        assert_eq!(witness_text(message, ""), "version=Version( 0, 1, ), token='a#b'; AssertionError: x");
    }

    #[test]
    fn hypothesis_failure_yields_input_and_reason_without_addresses() {
        assert_eq!(witness_text(MESSAGE, ""), "takes=[6]; AssertionError: took 6 with 5 left");
        let body = "    def test_x(a, b):\n>       assert a < b\nE       assert 2 < 1\nE       Falsifying example: test_x(\nE           a=2,\nE           b=1,\nE       )\n";
        assert_eq!(witness_text("", body), "a=2, b=1; assert 2 < 1");
        assert_eq!(
            witness_text("ZeroDivisionError: division by zero\nFalsifying example: test_x()", ""),
            "test_x(); ZeroDivisionError: division by zero"
        );
    }

    #[test]
    fn plain_failure_yields_its_first_line() {
        assert_eq!(witness_text("assert 1 == 2\n +  where 1 = f()", ""), "assert 1 == 2");
        assert_eq!(witness_text("", "E   AssertionError\n"), "AssertionError");
    }

    #[test]
    fn plugin_carries_the_budget_and_the_profile_name() {
        let text = plugin_text(Budget(40));
        assert!(
            text.contains("max_examples=40,") && text.contains("\"proob\",") && text.contains("database=None"),
            "{text}"
        );
    }
}
