//! `cargo xtask sim [scenario...]`: everything about proob that normally
//! depends on the outside world, run on this machine against local stand-ins
//! for it (a fixture GitHub, REST endpoints that refuse or rate-limit, a fake
//! release server, Claude Code's hook protocol, `act` for the runner).
//!
//!   world      a deterministic repo, the sandboxed binary, workflow runs
//!   github     the fixture GitHub and the REST stand-ins
//!   verdicts   expectations as data, observations, target/sim/verdicts.jsonl
//!   lifecycle  one property from declaration to amendment, real jobs
//!   honesty    adversarial worlds that must never render Holds
//!   soundness  240 worlds crossing approval, evidence, strength and real code truth
//!   score      `--score`: verdicts.jsonl scored with SV-COMP (cargo xtask bench soundness)
//!   requirement  a new requirement: uncovered code, a property for it, a superseding property
//!   shadow     shadow mode: open, run, report, adopt, clean, with no footprint
//!   external   the shell suites, act, dist plan
//!   outcome    a scenario's result and the summary table

mod external;
mod github;
mod honesty;
mod lifecycle;
mod outcome;
mod requirement;
mod score;
mod shadow;
mod soundness;
mod verdicts;
mod world;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use outcome::Outcome;
use verdicts::Log;
use world::Ctx;

struct Scenario {
    name: &'static str,
    about: &'static str,
    run: fn(&Ctx, &Log) -> Outcome,
}

const SCENARIOS: [Scenario; 9] = [
    Scenario {
        name: "lifecycle",
        about: "declare, merge, push and scheduled runs, a breaking PR, self-signed, amended (real jobs)",
        run: lifecycle,
    },
    Scenario {
        name: "honesty",
        about: "adversarial worlds that must never render Holds, and a control that must",
        run: honesty,
    },
    Scenario {
        name: "soundness",
        about: "240 worlds: approval x evidence x strength x real code truth, each recording rendered beside truth",
        run: soundness,
    },
    Scenario {
        name: "requirement",
        about: "a feature branch's new module listed as uncovered, a property for it, a changed requirement superseded (real jobs)",
        run: requirement,
    },
    Scenario {
        name: "shadow",
        about: "shadow mode on the Python fixture: open, a failing run, report, carry-over, adopt, clean; the repository untouched",
        run: |c, _| shadow::run_scenario(c),
    },
    Scenario {
        name: "shim",
        about: "tests/shim/test_shim.sh: the plugin shim against a fake release server",
        run: |c, _| external::shim(c),
    },
    Scenario {
        name: "hooks",
        about: "tests/hooks/test_hooks.sh: the Claude Code hooks",
        run: |c, _| external::hooks(c),
    },
    Scenario {
        name: "workflows",
        about: "act runs ci.yml's crate-graph and action-self-test jobs (needs Docker)",
        run: |c, _| external::workflows(c),
    },
    Scenario {
        name: "release",
        about: "dist plan releases proob alone, on the five targets (needs dist)",
        run: |c, _| external::release(c),
    },
];

fn lifecycle(ctx: &Ctx, log: &Log) -> Outcome {
    Outcome::of_verdicts(lifecycle::run(ctx, log), lifecycle::STEPS.len())
}

fn requirement(ctx: &Ctx, log: &Log) -> Outcome {
    Outcome::of_verdicts(requirement::run_scenario(ctx, log), requirement::STEPS.len())
}

fn soundness(ctx: &Ctx, log: &Log) -> Outcome {
    match soundness::run_scenario(ctx, log) {
        Ok(n) => Outcome::Ok(format!("{n} worlds recorded (scored by cargo xtask bench soundness)")),
        Err(e) => Outcome::Failed(e),
    }
}

fn honesty(ctx: &Ctx, log: &Log) -> Outcome {
    Outcome::of_verdicts(honesty::run_cases(ctx, log), honesty::CASES.len())
}

#[derive(Debug, PartialEq)]
enum Request<'a> {
    List,
    Score,
    Run(Vec<&'a Scenario>),
}

impl PartialEq for Scenario {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl std::fmt::Debug for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

/// `--list`, `--score`, nothing (every scenario), or scenario names; an unknown name is an error.
fn parse(args: &[String]) -> Result<Request<'static>, String> {
    match args {
        [flag] if flag == "--list" => Ok(Request::List),
        [flag] if flag == "--score" => Ok(Request::Score),
        [] => Ok(Request::Run(SCENARIOS.iter().collect())),
        names => names.iter().map(|n| find(n)).collect::<Result<_, _>>().map(Request::Run),
    }
}

fn find(name: &str) -> Result<&'static Scenario, String> {
    SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown scenario '{name}'; cargo xtask sim --list"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Request::List) => {
            SCENARIOS.iter().for_each(|s| println!("{:<11} {}", s.name, s.about));
            ExitCode::SUCCESS
        }
        Ok(Request::Score) => match score::score_file(&workspace_root().join("target/sim")) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(e) => fail(&e),
        },
        Ok(Request::Run(chosen)) => run_all(&chosen),
        Err(e) => {
            eprintln!("sim: {e}");
            ExitCode::from(2)
        }
    }
}

fn run_all(chosen: &[&Scenario]) -> ExitCode {
    let root = workspace_root();
    let ctx = Ctx { proob: root.join("target/debug/proob"), root };
    let log = match (ctx.proob.is_file(), Log::create(&ctx.root.join("target/sim/verdicts.jsonl"))) {
        (false, _) => return fail(&format!("{} is missing: cargo build --locked -p proob", ctx.proob.display())),
        (true, Err(e)) => return fail(&e),
        (true, Ok(log)) => log,
    };
    let rows: Vec<(&str, Outcome, f64)> = chosen.iter().map(|s| timed(s, &ctx, &log)).collect();
    print!("{}", outcome::table(&rows));
    println!("verdicts: {}", ctx.root.join("target/sim/verdicts.jsonl").display());
    match rows.iter().any(|(_, o, _)| o.is_failed()) {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    }
}

fn timed<'s>(s: &'s Scenario, ctx: &Ctx, log: &Log) -> (&'s str, Outcome, f64) {
    println!("== {} ==", s.name);
    let start = Instant::now();
    let outcome = (s.run)(ctx, log);
    (s.name, outcome, start.elapsed().as_secs_f64())
}

fn fail(why: &str) -> ExitCode {
    eprintln!("sim: {why}");
    ExitCode::FAILURE
}

/// The parent of this crate's directory, fixed at build time (as xtask does).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn nothing_selects_every_scenario_in_order() {
        let Ok(Request::Run(all)) = parse(&[]) else { panic!("runs") };
        let names: Vec<&str> = all.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            ["lifecycle", "honesty", "soundness", "requirement", "shadow", "shim", "hooks", "workflows", "release"]
        );
    }

    #[test]
    fn names_select_those_scenarios_and_unknown_names_fail() {
        let Ok(Request::Run(two)) = parse(&args(&["honesty", "lifecycle"])) else { panic!("runs") };
        assert_eq!(two.iter().map(|s| s.name).collect::<Vec<_>>(), ["honesty", "lifecycle"]);
        assert_eq!(parse(&args(&["--list"])), Ok(Request::List));
        assert_eq!(parse(&args(&["--score"])), Ok(Request::Score));
        assert!(parse(&args(&["lifecylce"])).unwrap_err().contains("unknown scenario 'lifecylce'"));
    }
}
