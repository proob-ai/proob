//! The composite scenario every end-to-end test reads: a repository whose
//! main holds one property per verdict class, served GitHub facts and
//! evidence runs through the fixture backend; and a pull request on top of it.
use super::*;
use serde_json::{Value, json};
use std::process::Command;

// ---------------------------------------------------------------------------
// The composite fixture: one property per verdict class
// ---------------------------------------------------------------------------

pub struct Composite {
    pub repo: TempRepo,
    pub fixture: Fixture,
    pub c1: String,
    pub c2: String,
    pub scenario: ScenarioBuilder,
}

const GOVERNED_AT_C1: [&str; 13] = [
    "proob/config.toml",
    ".github/workflows/proob-gate.yml",
    ".github/workflows/proob-evidence.yml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4A1.toml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4B2.toml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4C3.toml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4E5.toml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4F6.toml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4G7.toml",
    "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4J9.toml",
    "tests/prop_a.rs",
    "tests/prop_b.rs",
    "src/proof_j.rs",
];

fn waived_toml() -> String {
    format!(
        "{}\n[waiver]\nuntil = 2026-12-31\nissue = \"#12\"\nreason = \"known gap\"\n",
        property_toml(PROP_G, "G is waived", &["src/util.rs"], "test")
    )
}

/// main: c1 (the approved PR #7 introduces everything), c2 (a direct push
/// amends E and touches src/lib.rs); the tree adds D, an uncommitted proposal.
pub fn composite() -> Composite {
    composite_with(CONFIG_TOML)
}

/// `composite`, with `config` as the approved `proob/config.toml`.
pub fn composite_with(config: &str) -> Composite {
    let repo = TempRepo::new();
    let t1 = NOW - 10 * DAY;
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.write("src/util.rs", "pub fn g() {}\n");
    repo.write("tests/prop_a.rs", "// proob:01J9Z3W1V8Q4K7N2M5R8T1X4A1\n#[test] fn a() {}\n");
    repo.write("tests/prop_b.rs", "// proob:01J9Z3W1V8Q4K7N2M5R8T1X4B2\n#[test] fn b() {}\n");
    repo.write("src/proof_j.rs", "// proob:01J9Z3W1V8Q4K7N2M5R8T1X4J9\nproof fn j() {}\n");
    repo.write("proob/config.toml", config);
    repo.write(".github/workflows/proob-gate.yml", GATE_YML);
    repo.write(".github/workflows/proob-evidence.yml", EVIDENCE_YML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A holds", &["src/lib.rs"], "test"));
    repo.write(&property_path(PROP_B), &property_toml(PROP_B, "B is checked", &["src/util.rs"], "test"));
    repo.write(&property_path(PROP_C), &property_toml(PROP_C, "C has no check", &["src/lib.rs"], "test"));
    repo.write(&property_path(PROP_E), &property_toml(PROP_E, "E was amended", &["src/lib.rs"], "test"));
    repo.write(&property_path(PROP_F), &property_toml(PROP_F, "F matches nothing", &["nothing/**/*.zig"], "test"));
    repo.write(&property_path(PROP_G), &waived_toml());
    repo.write(&property_path(PROP_J), &property_toml(PROP_J, "J is proved", &["src/lib.rs"], "proof"));
    let c1 = repo.commit("introduce proob (PR #7)", t1);
    repo.write(
        &property_path(PROP_E),
        &property_toml(PROP_E, "E was amended", &["src/lib.rs"], "test").replace("always.", "always, amended."),
    );
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let c2 = repo.commit("direct push: amend E, touch lib", NOW - 5 * DAY);
    repo.write(&property_path(PROP_D), &property_toml(PROP_D, "D is proposed", &["src/lib.rs"], "test"));

    let mut scenario = ScenarioBuilder::new();
    scenario
        .merged_pr(7, ALICE, &c1, &GOVERNED_AT_C1)
        .review(7, 501, BOB, "APPROVED", &c1)
        .governance(true, true, true, true)
        .issue(12, true);
    for path in GOVERNED_AT_C1 {
        let blob = repo.blob(&c1, path);
        scenario.blob(&c1, path, Some(&blob));
    }
    let bindings = json!({
        PROP_A: { "files": ["tests/prop_a.rs"] },
        PROP_B: { "files": ["tests/prop_b.rs"] },
        PROP_J: { "files": ["src/proof_j.rs"] },
    });
    let fixture = Fixture::new(&scenario.build(), bindings.as_object());
    let mut composite = Composite { repo, fixture, c1, c2, scenario };
    composite.add_evidence();
    composite
}

impl Composite {
    /// The scheduled run on main at c2: A checked and scored, B checked only,
    /// J (a proof) verified and scored.
    fn add_evidence(&mut self) {
        let a = hashes_of(&self.repo, &self.fixture, PROP_A);
        let b = hashes_of(&self.repo, &self.fixture, PROP_B);
        let j = hashes_of(&self.repo, &self.fixture, PROP_J);
        let blob = self.repo.blob("main", ".github/workflows/proob-evidence.yml");
        let run = RunMeta { run_id: 101, head_sha: &self.c2, workflow_blob: &blob, at: NOW - DAY };
        let records = vec![
            record(PROP_A, "check", &a, json!({ "result": "no_counterexample", "budget": 256 }), &run),
            record(
                PROP_A,
                "score",
                &a,
                json!({ "result": "scored", "killed": 3, "survived": 1, "timeout": 0, "unviable": 0 }),
                &run,
            ),
            record(PROP_B, "check", &b, json!({ "result": "no_counterexample", "budget": 256 }), &run),
            record(PROP_J, "proof", &j, json!({ "result": "verified", "rlimit": 10 }), &run),
            record(
                PROP_J,
                "score",
                &j,
                json!({ "result": "scored", "killed": 3, "survived": 1, "timeout": 0, "unviable": 0 }),
                &run,
            ),
        ];
        self.scenario.evidence_run(101, &self.c2.clone(), "schedule", "2026-10-27T00:00:00Z", records);
        self.fixture.write_scenario(&self.scenario.build());
    }

    pub fn proob(&self) -> Command {
        let mut cmd = self.repo.proob();
        cmd.env("PROOB_FIXTURE_DIR", self.fixture.path());
        cmd
    }
}

pub struct Checked {
    pub c: Composite,
    pub c3: String,
    pub c4: String,
}

/// A `pr` branch off main: c3 touches src/lib.rs and adds H; c4 edits H and
/// relaxes the gate in the head's config. PR #8, reviewed by bob at c3.
pub fn with_pr() -> Checked {
    let mut c = composite();
    c.repo.remove(&property_path(PROP_D));
    c.repo.branch("pr");
    c.repo.write("src/lib.rs", "pub fn f() -> u8 { 2 }\n");
    c.repo.write(&property_path(PROP_H), &property_toml(PROP_H, "H is new", &["src/util.rs"], "test"));
    let c3 = c.repo.commit("touch lib, add H", NOW - 2 * DAY);
    c.repo.write(
        &property_path(PROP_H),
        &property_toml(PROP_H, "H is new", &["src/util.rs"], "test").replace("always.", "always, v2."),
    );
    c.repo
        .write("proob/config.toml", &CONFIG_TOML.replace("broken_affected = \"block\"", "broken_affected = \"warn\""));
    let c4 = c.repo.commit("edit H, relax the gate", NOW - DAY);
    let h = property_path(PROP_H);
    c.scenario
        .open_pr(8, ALICE, &c4)
        .pr_commit(8, ALICE, &c3, &["src/lib.rs", &h], &[&c.c2.clone()])
        .pr_commit(8, ALICE, &c4, &[&h, "proob/config.toml"], &[&c3])
        .review(8, 801, BOB, "APPROVED", &c3)
        .blob(&c3, &h, Some(&c.repo.blob(&c3, &h)))
        .blob(&c3, "proob/config.toml", Some(&c.repo.blob(&c3, "proob/config.toml")))
        .blob(&c4, &h, Some(&c.repo.blob(&c4, &h)))
        .blob(&c4, "proob/config.toml", Some(&c.repo.blob(&c4, "proob/config.toml")));
    c.fixture.write_scenario(&c.scenario.build());
    Checked { c, c3, c4 }
}

impl Checked {
    pub fn check(&self, results: Option<&str>) -> Command {
        let mut cmd = self.c.proob();
        cmd.args(["check", "--base", "main", "--head", "pr", "--pr", "8"]);
        if let Some(text) = results {
            let path = self.c.repo.root().join("results.jsonl");
            std::fs::write(&path, text).expect("results");
            cmd.arg("--results").arg(path);
        }
        cmd
    }

    pub fn results(&self, outcome: Value) -> String {
        let a = hashes_of(&self.c.repo, &self.c.fixture, PROP_A);
        result_line(PROP_A, "check", &a, outcome) + "\n"
    }
}
