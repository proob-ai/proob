//! `check` for a TLA+ binding: TLC on a copy of the binding set's modules
//! with a cfg that checks only the bound name, classified by
//! `proob_facts::parsers::tlc`. The classifier decides, never the exit
//! code: a run is `NoViolation` only when TLC printed its completion line, no
//! error, and exited 0.
use crate::argv::Command;
use crate::tla::tools::{Resolution, Tools, resolve};
use crate::tla::{CheckRef, TlaAdapter, cfg, discover::cfg_of, parse_check_ref};
use crate::{AdapterError, Binding, Check, CheckOutcome, ToolVersions, WITNESS_CAP, cap, exit_text};
use proob_facts::parsers::tlc::{Class, classify, version};
use proob_spawn::{Output, SpawnError};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The wall-clock limit on one run.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// The generated cfg, beside the spec: TLC finds a cfg only in the spec's directory.
pub const CFG_NAME: &str = "proob-check.cfg";
const REASON_CAP: usize = 600;

/// `java -XX:+UseParallelGC -cp <jar> tlc2.TLC -config <cfg> -workers 1 -fp 0 -seed 0 -metadir <dir> <spec>`.
pub fn plan(tools: &Tools, spec: &Path, cfg: &Path, metadir: &Path, timeout: Duration) -> Command {
    let dir = spec.parent().unwrap_or(Path::new("/"));
    Command::new(&tools.java, timeout)
        .arg("-XX:+UseParallelGC")
        .own("-cp", &tools.jar)
        .arg("tlc2.TLC")
        .own("-config", cfg)
        .arg("-workers")
        .arg("1")
        .arg("-fp")
        .arg("0")
        .arg("-seed")
        .arg("0")
        .own("-metadir", metadir)
        .own_path(spec)
        .cwd(dir)
}

pub fn run(adapter: &TlaAdapter, repo: &Path, binding: &Binding) -> Result<Check, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let tools = match resolve(adapter.tool_env()) {
        Resolution::Ready(tools) => tools,
        Resolution::Missing { reason, versions } => {
            return Ok(Check { outcome: CheckOutcome::Errored { reason }, tool_versions: versions });
        }
    };
    let lab = Lab::new(adapter, repo, binding, &r)?;
    let (outcome, versions) = match lab.run(adapter, &tools, TIMEOUT) {
        Ok(out) => (judge(&out, r.name.as_str()), versions_of(&tools, &out)),
        Err(e) => (stopped(&e), tools.versions.clone()),
    };
    Ok(Check { outcome, tool_versions: versions })
}

/// The tools' versions with the TLC release the run printed.
pub fn versions_of(tools: &Tools, out: &Output) -> ToolVersions {
    let tlc = version(&String::from_utf8_lossy(&out.stdout)).map(|v| ("tlc".to_string(), v));
    tools.versions.iter().cloned().chain(tlc).collect()
}

/// The outcome of a run that never finished: always errored.
pub fn stopped(e: &SpawnError) -> CheckOutcome {
    let reason = match e {
        SpawnError::TimedOut { after, .. } => format!(
            "TLC timed out after {} minutes, the adapter's limit on one run; shrink the model's constants",
            after.as_secs() / 60
        ),
        SpawnError::OutOfMemory { limit_bytes, .. } => out_of_memory(*limit_bytes),
        e => format!("TLC did not run: {e}"),
    };
    CheckOutcome::Errored { reason }
}

/// Why a run proob stopped for memory has no verdict.
pub fn out_of_memory(limit_bytes: u64) -> String {
    format!(
        "TLC used more than {} MB and was stopped (out of memory); raise {} or shrink the model's constants",
        limit_bytes >> 20,
        proob_spawn::MEMORY_LIMIT_VAR
    )
}

/// The outcome of one finished run checking `name`.
pub fn judge(out: &Output, name: &str) -> CheckOutcome {
    let class = classify(&String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr));
    outcome(class, out.code, name)
}

/// The outcome table, row by row.
pub fn outcome(class: Class, code: Option<i32>, name: &str) -> CheckOutcome {
    let errored = |reason: String| CheckOutcome::Errored { reason: cap(&reason, REASON_CAP) };
    let violation =
        |what: String, trace: String| CheckOutcome::Violation { trace: cap(&format!("{what}\n{trace}"), WITNESS_CAP) };
    match class {
        Class::NoError if code == Some(0) => CheckOutcome::NoViolation,
        Class::NoError => errored(format!("unrecognised TLC output: no error found but exit {}", exit_text(code))),
        Class::InvariantViolated { .. } | Class::TemporalViolated { .. } if code == Some(0) => {
            errored("unrecognised TLC output: a violation but exit code 0".to_string())
        }
        Class::InvariantViolated { invariant, trace } if invariant == name => {
            violation(format!("Invariant {name} is violated."), trace)
        }
        Class::InvariantViolated { invariant, .. } => {
            errored(format!("TLC reported invariant `{invariant}` violated, but only `{name}` was checked"))
        }
        Class::TemporalViolated { trace } => violation(format!("Temporal property {name} is violated."), trace),
        Class::Deadlock { trace } => errored(format!(
            "deadlock reached: a reachable state has no next state, so TLC stopped before checking `{name}` everywhere; fix the spec, or add `CHECK_DEADLOCK FALSE` to the cfg if stopping is intended. {}",
            trace.replace('\n', " ")
        )),
        Class::ParseError { detail } => errored(format!("TLC could not parse the spec: {detail}")),
        Class::SemanticError { detail } => errored(format!("TLC rejected the spec (semantic error): {detail}")),
        Class::OutOfMemory => errored("TLC ran out of Java heap; shrink the model's constants".to_string()),
        Class::Unrecognised { .. } if code.is_none() => {
            errored("TLC was killed by a signal before it finished".to_string())
        }
        Class::Unrecognised { detail } => errored(format!("unrecognised TLC output: {detail}")),
    }
}

/// A temporary directory with copies of the binding set's modules and the
/// generated cfg, where TLC runs (and where `score` writes its mutants).
pub(crate) struct Lab {
    dir: tempfile::TempDir,
    /// The bound module's copy.
    pub(crate) spec: PathBuf,
    runs: Cell<u32>,
}

impl Lab {
    pub(crate) fn new(adapter: &TlaAdapter, repo: &Path, binding: &Binding, r: &CheckRef) -> Result<Lab, AdapterError> {
        let io = |path: &Path, e: std::io::Error| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() };
        let dir = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
        let model = dir.path().join("model");
        std::fs::create_dir(&model).map_err(|e| io(&model, e))?;
        for file in binding.files.iter().filter(|f| f.extension().is_some_and(|e| e == "tla")) {
            let to = model.join(file.file_name().unwrap_or_default());
            std::fs::copy(repo.join(file), &to).map_err(|e| io(&repo.join(file), e))?;
        }
        let cfg_path = adapter.root_in(repo).join(cfg_of(Path::new(r.module.as_str())));
        let original = std::fs::read_to_string(&cfg_path).map_err(|e| io(&cfg_path, e))?;
        let generated = model.join(CFG_NAME);
        std::fs::write(&generated, cfg::only(&original, r.name.as_str())).map_err(|e| io(&generated, e))?;
        let name = Path::new(r.module.as_str()).file_name().unwrap_or_default();
        Ok(Lab { spec: model.join(name), dir, runs: Cell::new(0) })
    }

    /// One TLC run in the lab, with a metadir of its own (TLC refuses to reuse one).
    pub(crate) fn run(&self, adapter: &TlaAdapter, tools: &Tools, timeout: Duration) -> Result<Output, SpawnError> {
        let n = self.runs.get() + 1;
        self.runs.set(n);
        let metadir = self.dir.path().join(format!("meta-{n}"));
        plan(tools, &self.spec, &self.spec.with_file_name(CFG_NAME), &metadir, timeout).envs(adapter.tool_env()).run()
    }
}
