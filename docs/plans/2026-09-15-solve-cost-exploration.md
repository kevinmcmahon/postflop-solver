# Solve Cost Exploration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lower dollars per accepted hand for hand-ranger's 200bb round by landing, in order, a regression gate against the fork, solve parameters, line derivation from saved games, and a cost model, so every later memory reduction is measured against a known-good build and priced before it is built.

**Architecture:** Engine changes live in postflop-solver (this fork) and are guarded by its byte-exact golden suite and criterion benches. Protocol, adapter, gate script, cost runner, and runbook live in hand-ranger; the generator change (tree/0.2, configuration record) and the superseding ADR for ADR-0008 are Kevin's and are handoffs, not tasks. Phase 3 (memory reductions) gets its own plan once phase 2 has numbers.

**Tech Stack:** Rust 1.96 (edition 2024 on the fork after the modernize branch merges), cargo `[patch]` overrides, bincode 2.0.0-rc.3 save files, hand-ranger's Go generator (`hand-ranger-gen`), bash and Python 3 for the gate and cost runner, criterion.

**Spec:** `docs/specs/2026-09-15-solve-cost-exploration-design.md` in postflop-solver. Read it first; every task argues from it.

## Global Constraints

- Two repositories. postflop-solver: `/Users/kevin/sync/projects/poker/postflop-solver` (fork of b-inary, remote `git@github.com:kevinmcmahon/postflop-solver.git`). hand-ranger: `/Users/kevin/sync/projects/poker/hand-ranger`; the local checkout is 68 commits behind `origin/main` at the time of writing, so `git pull --ff-only` first. Read each repo's `AGENTS.md` before changing it.
- Isolation: one worktree per repository under `.worktrees/<branch>`; branch names `solve-params`, `save-derive`, `cost-model` in postflop-solver and `fork-gate`, `protocol-0.2`, `cost-runner` in hand-ranger. Never push. Linear history; amend review fixes into the task's commit while unpushed. Gitmoji conventional commits.
- postflop-solver behaviour oracle for every engine task: `RUSTFLAGS="--deny warnings" cargo test --release --features zstd --test golden` passes with `UPDATE_GOLDEN` unset, and every pre-existing golden file is byte-identical after the task (`git diff --stat tests/golden/` shows only files the task adds). `UPDATE_GOLDEN=1` is used only to create a new scenario's file, filtered to that test name.
- postflop-solver gates (CI uses `RUSTFLAGS=--deny warnings`, `RUSTDOCFLAGS=--deny warnings`): `cargo build --release --features zstd`; `cargo build --release --no-default-features --features bincode`; `cargo test --release --features zstd`; `cargo clippy --release --features zstd --all-targets -- -A clippy::needless_range_loop`; `cargo fmt --all --check`; `cargo doc --release`; `cargo bench --bench solve --no-run --features zstd`; `cargo +nightly build --release --features custom-alloc`; `cargo +nightly test --release --features custom-alloc -- --test-threads 1`.
- Gate pass rule (spec 1a), all three for both exports: every graded step within 0.03 total variation of `dataset/hands/hr-0002/hand.json`'s references; final exploitability at or below 2.75 chips; `derivation_checks_failed` empty. Recorded margin: worst committed step 0.0233 at cp-11.
- Protocol: `hand-ranger.solver-protocol/0.2`; the adapter must keep reading 0.1 and produce byte-identical exports for 0.1 requests (regenerating a committed hand from its saved export must not change).
- Solve parameter presets, exact: `current` = alpha 1.5, beta 0, gamma 3, restart at powers of four; `paper` = alpha 1.5, beta 0, gamma 2, no restart. `current` must reproduce today's discounting bit for bit.
- hr-0002 facts: J♠9♠6♦ tree at 100bb, starting pot 550, 4.0 GB compressed, request `dataset/hands/hr-0002/solver-request.json` (600 iterations, target 0, check every 100, compression on), path of 11 steps. Committed exports: `solver-export.json` (600) and `stability-export.json` (1,000). Both gate solves take about 95 minutes at 4 threads.
- Prose in docs and comments: plain English, evergreen, no em dashes, no filler openers, no bold-first bullets. Lowercase hyphenated file names.
- Saved games and derived exports never enter git.

---

## Phase 1a: regression gate (hand-ranger, branch `fork-gate`)

### Task 1: Adapter records the solver revision it was built from

**Files:**
- Modify: `adapters/postflop-solver/src/main.rs:25-26` (constants) and the `SolverInfo` construction near line 498.
- Modify: `adapters/postflop-solver/PROTOCOL.md` (one paragraph under Export).

**Interfaces:**
- Produces: build-time variables `HR_SOLVER_REPOSITORY` and `HR_SOLVER_REVISION`, read with `option_env!`, falling back to the constants. Task 2's gate script sets them.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `adapters/postflop-solver/src/main.rs`:

```rust
#[cfg(test)]
mod solver_info_tests {
    use super::*;

    #[test]
    fn solver_info_uses_build_variables_when_set() {
        let info = solver_info();
        let expected_repo = option_env!("HR_SOLVER_REPOSITORY").unwrap_or(SOLVER_REPO);
        let expected_rev = option_env!("HR_SOLVER_REVISION").unwrap_or(SOLVER_REV);
        assert_eq!(info.repository, expected_repo);
        assert_eq!(info.revision, expected_rev);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run from `adapters/postflop-solver`: `cargo test solver_info_uses_build_variables_when_set`
Expected: FAIL, `solver_info` not found.

- [ ] **Step 3: Implement**

Replace the inline `SolverInfo { ... }` construction at the export site with a call to a new function, and add the function next to the constants:

```rust
fn solver_info() -> SolverInfo {
    SolverInfo {
        name: "postflop-solver".into(),
        repository: option_env!("HR_SOLVER_REPOSITORY").unwrap_or(SOLVER_REPO).into(),
        revision: option_env!("HR_SOLVER_REVISION").unwrap_or(SOLVER_REV).into(),
        license: SOLVER_LICENSE.into(),
        features: if cfg!(feature = "parallel") { vec!["parallel".into()] } else { vec![] },
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        rustc_version: option_env!("HR_RUSTC_VERSION").unwrap_or("unknown").into(),
    }
}
```

Add a comment above the constants: the constants describe the committed pin; a build made against another solver (the fork gate does this with a cargo patch override) sets the two variables so the export names what was actually built.

- [ ] **Step 4: Run the test twice**

`cargo test solver_info_uses_build_variables_when_set` and `HR_SOLVER_REVISION=abc123 HR_SOLVER_REPOSITORY=https://example.invalid cargo test solver_info_uses_build_variables_when_set` (the second forces a rebuild because `option_env!` is a compile-time read). Expected: PASS both.

- [ ] **Step 5: Document**

In `PROTOCOL.md` under Export add: `solver.repository` and `solver.revision` name the solver the adapter was built against; a build against a patched solver must set `HR_SOLVER_REPOSITORY` and `HR_SOLVER_REVISION` at build time, otherwise the export names the committed pin.

- [ ] **Step 6: Commit**

```bash
git add adapters/postflop-solver/src/main.rs adapters/postflop-solver/PROTOCOL.md
git commit -m "feat: ✨ adapter records the solver revision it was built against"
```

### Task 2: Fork gate script

**Files:**
- Create: `adapters/postflop-solver/gate.sh`
- Create: `adapters/postflop-solver/GATE.md`

**Interfaces:**
- Consumes: Task 1's build variables; `hand-ranger-gen stability --manifest --inputs --hand --reuse-export` (writes `stability.json`, `stability-export.json`, `stability-stderr.log` beside `hand.json`; read `generator/cmd/hand-ranger-gen/main.go` for the exact flags on `origin/main`); `hand-ranger-gen generate --manifest --inputs --out --reuse-export`; `docs/evidence/hand-02/stability/compare.py`.
- Produces: `gate.sh [--solver-path DIR | --solver-rev SHA --solver-repo URL] [--request FILE] [--hand DIR] [--threads N] [--out DIR]`, exit 0 on pass, 1 on fail, 2 on usage or build error.

- [ ] **Step 1: Read the two tools' contracts**

Read `generator/cmd/hand-ranger-gen/main.go` (`stability` and `generate` subcommands), `generator/internal/generate/stability.go` (which files it writes, what `derivation_checks_failed` and `steps[].tv` contain), and `docs/evidence/hand-02/stability/compare.py` (its arguments and output). Record in `GATE.md` which subcommand grades which export: the 1,000-iteration export is the stability request's export and goes to `stability --reuse-export`; the 600-iteration export is the accepted request's export and goes to `generate --reuse-export --out <tmp>`, whose regenerated `hand.json` is compared step by step with the committed `hand.json` by `compare.py`. If `compare.py` does not print per-step total variation, extend it to do so in this task (it is evidence tooling, not dataset content).

- [ ] **Step 2: Write the script**

```bash
#!/usr/bin/env bash
# Fork gate: build the adapter against a given postflop-solver, solve hr-0002 twice,
# and grade both exports against the committed references. See GATE.md.
set -euo pipefail

PIN_URL="https://github.com/b-inary/postflop-solver"
HAND_DIR="dataset/hands/hr-0002"
REQUEST="$HAND_DIR/solver-request.json"
MANIFEST="dataset/inputs/manifests/hand-02.json"
INPUTS="dataset/inputs"
THREADS="${RAYON_NUM_THREADS:-4}"
OUT=""
SOLVER_PATH=""; SOLVER_REV=""; SOLVER_REPO=""
TV_LIMIT="0.03"; EXPLOIT_LIMIT="2.75"

usage() { echo "usage: gate.sh (--solver-path DIR | --solver-rev SHA --solver-repo URL) [--request FILE] [--hand DIR] [--threads N] [--out DIR]" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --solver-path) SOLVER_PATH="$2"; shift 2;;
    --solver-rev) SOLVER_REV="$2"; shift 2;;
    --solver-repo) SOLVER_REPO="$2"; shift 2;;
    --request) REQUEST="$2"; shift 2;;
    --hand) HAND_DIR="$2"; shift 2;;
    --threads) THREADS="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    *) usage;;
  esac
done
[ -z "$OUT" ] && OUT="$(mktemp -d "${TMPDIR:-/tmp}/fork-gate.XXXXXX")"
mkdir -p "$OUT"

# Resolve what is being built and make the build embed it.
if [ -n "$SOLVER_PATH" ]; then
  [ -z "$(git -C "$SOLVER_PATH" status --porcelain)" ] || { echo "solver worktree is dirty; commit first" >&2; exit 2; }
  SOLVER_REV="$(git -C "$SOLVER_PATH" rev-parse HEAD)"
  SOLVER_REPO="$(git -C "$SOLVER_PATH" remote get-url origin)"
  PATCH="patch.\"$PIN_URL\".postflop-solver.path=\"$(cd "$SOLVER_PATH" && pwd)\""
elif [ -n "$SOLVER_REV" ] && [ -n "$SOLVER_REPO" ]; then
  PATCH="patch.\"$PIN_URL\".postflop-solver={git=\"$SOLVER_REPO\",rev=\"$SOLVER_REV\"}"
else
  usage
fi

echo "building adapter against $SOLVER_REPO @ $SOLVER_REV"
( cd adapters/postflop-solver && \
  HR_SOLVER_REPOSITORY="$SOLVER_REPO" HR_SOLVER_REVISION="$SOLVER_REV" HR_RUSTC_VERSION="$(rustc --version)" \
  cargo build --release --features parallel --config "$PATCH" )
ADAPTER="adapters/postflop-solver/target/release/hand-ranger-postflop-adapter"
BIN="$(ls generator/hand-ranger-gen 2>/dev/null || true)"
[ -x "$BIN" ] || { ( cd generator && go build -o hand-ranger-gen ./cmd/hand-ranger-gen ); BIN="generator/hand-ranger-gen"; }

# Two solves: the request as committed, and the same request at 1,000 iterations.
python3 - "$REQUEST" "$OUT/request-1000.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1])); r["solve"]["max_iterations"] = 1000
json.dump(r, open(sys.argv[2], "w"), indent=1)
PY
for pair in "600:$REQUEST" "1000:$OUT/request-1000.json"; do
  n="${pair%%:*}"; req="${pair#*:}"
  echo "solving $n iterations with $THREADS threads"
  /usr/bin/time -p env RAYON_NUM_THREADS="$THREADS" "$ADAPTER" < "$req" > "$OUT/export-$n.json" 2> "$OUT/stderr-$n.log"
  rev="$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['solver']['revision'])" "$OUT/export-$n.json")"
  [ "$rev" = "$SOLVER_REV" ] || { echo "FAIL: export-$n names revision $rev, built $SOLVER_REV" >&2; exit 1; }
done

# Grade. The 1,000 export is the stability request's export; the 600 export regenerates the hand.
WORK="$OUT/hand"; rm -rf "$WORK"; cp -R "$HAND_DIR" "$WORK"
"$BIN" stability --manifest "$MANIFEST" --inputs "$INPUTS" --adapter "$ADAPTER" --hand "$WORK" --reuse-export "$OUT/export-1000.json"
"$BIN" generate --manifest "$MANIFEST" --inputs "$INPUTS" --adapter "$ADAPTER" --out "$OUT/regen" --reuse-export "$OUT/export-600.json"
python3 docs/evidence/hand-02/stability/compare.py "$HAND_DIR/hand.json" "$OUT/regen/hand.json" > "$OUT/compare-600.txt"

python3 - "$OUT" "$TV_LIMIT" "$EXPLOIT_LIMIT" <<'PY'
import json, sys, re
out, tv_limit, ex_limit = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
ok = True
def line(s): print(s)
st = json.load(open(f"{out}/hand/stability.json"))
line("export-1000 (stability tool)")
for s in st["steps"]:
    flag = "ok" if s["tv"] <= tv_limit else "FAIL"; ok &= s["tv"] <= tv_limit
    line(f"  {s['checkpoint_id']:>6} {s['action_type']:>5} tv={s['tv']:.4f} {flag}")
if st["derivation_checks_failed"]:
    ok = False; line(f"  derivation_checks_failed: {st['derivation_checks_failed']}")
for n in ("600", "1000"):
    ex = json.load(open(f"{out}/export-{n}.json"))["solve_result"]["exploitability"]
    flag = "ok" if ex <= ex_limit else "FAIL"; ok &= ex <= ex_limit
    line(f"export-{n} exploitability={ex:.4f} chips (limit {ex_limit}) {flag}")
line("export-600 (generate + compare)")
for m in re.finditer(r"(cp-\d+)\D+tv=([0-9.]+)", open(f"{out}/compare-600.txt").read()):
    tv = float(m.group(2)); flag = "ok" if tv <= tv_limit else "FAIL"; ok &= tv <= tv_limit
    line(f"  {m.group(1):>6} tv={tv:.4f} {flag}")
line("PASS" if ok else "FAIL"); sys.exit(0 if ok else 1)
PY
```

Adjust the `compare.py` invocation and the regex to that script's real output once Step 1 has established it; the gate must fail, not pass, when it cannot parse a step.

- [ ] **Step 3: Quick check on a small request**

Make `chmod +x gate.sh`. Run `./adapters/postflop-solver/gate.sh --solver-rev 9d1509fe5077d019825f833eed04b16d342dfda1 --solver-repo https://github.com/b-inary/postflop-solver --request docs/research/tree-probe-200bb/j96-200-tree01.json --out /tmp/gate-smoke` after editing a copy of that request to `plan_only: false`, `max_iterations: 5`, and `max_memory_bytes` large enough. Expected: the two solves run, the revision check passes, grading fails on total variation (5 iterations), and the script exits 1 with a table. This proves the plumbing without a 95-minute run.

- [ ] **Step 4: Canonical run against the unchanged fork**

`./adapters/postflop-solver/gate.sh --solver-path /Users/kevin/sync/projects/poker/postflop-solver --threads 4 --out /tmp/gate-fork-main`
Expected: PASS, with every step at or below 0.03 and both exploitabilities at or below 2.75. Record the table, the wall times from `stderr-*.log`, and the fork revision in `GATE.md` under "Runs". If any step fails, stop and report the table; that is the finding the gate exists to produce.

- [ ] **Step 5: Write GATE.md**

Sections: what the gate proves; how to run (both forms); what each subcommand grades; the pass rule with the three conditions and the recorded margin; where outputs go; the run log.

- [ ] **Step 6: Commit**

```bash
git add adapters/postflop-solver/gate.sh adapters/postflop-solver/GATE.md docs/evidence/hand-02/stability/compare.py
git commit -m "feat: ✨ fork gate script grading hr-0002 against committed references"
```

**Handoff to Kevin after Task 2 passes:** the superseding ADR for ADR-0008 (proposal until accepted), stating the fork URL and revision, that the 200bb round is solved entirely on the fork build that passed the gate, and that one adapter pin serves one version of a set. The repoint of `adapters/postflop-solver/Cargo.toml` happens after acceptance and is a one-line change plus a gate re-run with `--solver-rev`.

---

## Phase 1b: solve parameters (postflop-solver, branch `solve-params`)

### Task 3: `SolveParams` with bit-exact `current` preset

**Files:**
- Modify: `src/solver.rs:10-38` (`DiscountParams`), `solve` and `solve_step` signatures.
- Test: unit tests at the bottom of `src/solver.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub enum RestartSchedule { PowersOfFour, None, At(Vec<u32>) }
  pub struct SolveParams { pub alpha: f64, pub beta: f64, pub gamma: u32, pub restart: RestartSchedule }
  impl SolveParams { pub fn current() -> Self; pub fn paper() -> Self }
  impl Default for SolveParams  // current()
  pub fn solve_with_params<T: Game>(game: &mut T, max_num_iterations: u32, target_exploitability: f32, print_progress: bool, params: &SolveParams) -> f32
  pub fn solve_step_with_params<T: Game>(game: &T, current_iteration: u32, params: &SolveParams)
  ```
  `solve` and `solve_step` keep their signatures and call the new functions with `SolveParams::current()`.

- [ ] **Step 1: Write the failing test**

Append to `src/solver.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The discounting the engine used before `SolveParams` existed, kept verbatim as the oracle.
    fn legacy(current_iteration: u32) -> (f32, f32, f32) {
        let nearest_lower_power_of_4 = match current_iteration {
            0 => 0,
            x => 1 << ((x.leading_zeros() ^ 31) & !1),
        };
        let t_alpha = (current_iteration as i32 - 1).max(0) as f64;
        let t_gamma = (current_iteration - nearest_lower_power_of_4) as f64;
        let pow_alpha = t_alpha * t_alpha.sqrt();
        let pow_gamma = (t_gamma / (t_gamma + 1.0)).powi(3);
        ((pow_alpha / (pow_alpha + 1.0)) as f32, 0.5, pow_gamma as f32)
    }

    #[test]
    fn current_preset_matches_legacy_discounting_bit_for_bit() {
        let params = SolveParams::current();
        for t in 0..5000 {
            let d = DiscountParams::new(t, &params);
            let (a, b, g) = legacy(t);
            assert_eq!(d.alpha_t.to_bits(), a.to_bits(), "alpha at {t}");
            assert_eq!(d.beta_t.to_bits(), b.to_bits(), "beta at {t}");
            assert_eq!(d.gamma_t.to_bits(), g.to_bits(), "gamma at {t}");
        }
    }

    #[test]
    fn paper_preset_never_restarts_and_uses_square() {
        let params = SolveParams::paper();
        let d = DiscountParams::new(16, &params);
        let t = 16.0f64;
        assert_eq!(d.gamma_t, ((t / (t + 1.0)).powi(2)) as f32);
        assert!(DiscountParams::new(64, &params).gamma_t > 0.9);
    }

    #[test]
    fn explicit_restart_list_resets_at_listed_iterations() {
        let params = SolveParams { restart: RestartSchedule::At(vec![100, 300]), ..SolveParams::current() };
        assert_eq!(DiscountParams::new(100, &params).gamma_t, 0.0);
        assert_eq!(DiscountParams::new(300, &params).gamma_t, 0.0);
        assert!(DiscountParams::new(200, &params).gamma_t > 0.0);
        assert_eq!(DiscountParams::new(0, &params).gamma_t, 0.0);
    }
}
```

- [ ] **Step 2: Run to verify failure**

`cargo test --release --lib solver::tests` Expected: compile error, `SolveParams` not defined.

- [ ] **Step 3: Implement**

Replace the `DiscountParams` block in `src/solver.rs` with:

```rust
/// When the cumulative strategy is reset during solving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartSchedule {
    /// Reset at iterations 1, 4, 16, 64, 256, ... (the engine's long-standing behaviour).
    PowersOfFour,
    /// Never reset.
    None,
    /// Reset at exactly these iterations (ascending).
    At(Vec<u32>),
}

/// Discounted CFR parameters. `alpha` and `beta` are the exponents applied to positive and
/// negative regrets, `gamma` the exponent applied to the cumulative strategy, and `restart`
/// decides when the cumulative strategy is zeroed.
#[derive(Debug, Clone, PartialEq)]
pub struct SolveParams {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: u32,
    pub restart: RestartSchedule,
}

impl SolveParams {
    /// The engine's default: alpha 1.5, beta 0, gamma 3, restart at powers of four.
    pub fn current() -> Self {
        Self { alpha: 1.5, beta: 0.0, gamma: 3, restart: RestartSchedule::PowersOfFour }
    }

    /// The Discounted CFR paper's recommendation: alpha 1.5, beta 0, gamma 2, no restart.
    pub fn paper() -> Self {
        Self { alpha: 1.5, beta: 0.0, gamma: 2, restart: RestartSchedule::None }
    }
}

impl Default for SolveParams {
    fn default() -> Self {
        Self::current()
    }
}

struct DiscountParams {
    alpha_t: f32,
    beta_t: f32,
    gamma_t: f32,
}

impl DiscountParams {
    fn new(current_iteration: u32, params: &SolveParams) -> Self {
        let restart_base = match &params.restart {
            // 0, 1, 4, 16, 64, 256, ...
            RestartSchedule::PowersOfFour => match current_iteration {
                0 => 0,
                x => 1 << ((x.leading_zeros() ^ 31) & !1),
            },
            RestartSchedule::None => 0,
            RestartSchedule::At(points) => points
                .iter()
                .copied()
                .filter(|&p| p <= current_iteration)
                .max()
                .unwrap_or(0),
        };

        let t_alpha = (current_iteration as i32 - 1).max(0) as f64;
        let t_gamma = (current_iteration - restart_base) as f64;

        // The 1.5 case keeps the exact expression the engine has always used so that
        // `SolveParams::current()` reproduces old results bit for bit.
        let pow_alpha = if params.alpha == 1.5 { t_alpha * t_alpha.sqrt() } else { t_alpha.powf(params.alpha) };
        let beta_t = if params.beta == 0.0 {
            0.5
        } else {
            let pow_beta = t_alpha.powf(params.beta);
            (pow_beta / (pow_beta + 1.0)) as f32
        };
        let pow_gamma = (t_gamma / (t_gamma + 1.0)).powi(params.gamma as i32);

        Self {
            alpha_t: (pow_alpha / (pow_alpha + 1.0)) as f32,
            beta_t,
            gamma_t: pow_gamma as f32,
        }
    }
}
```

Then rename the existing `solve` body to `solve_with_params` adding `params: &SolveParams` as the last parameter and replacing `DiscountParams::new(t)` with `DiscountParams::new(t, params)`; same for `solve_step` to `solve_step_with_params`. Add thin wrappers:

```rust
/// Performs Discounted CFR with `SolveParams::current()`. See `solve_with_params`.
pub fn solve<T: Game>(game: &mut T, max_num_iterations: u32, target_exploitability: f32, print_progress: bool) -> f32 {
    solve_with_params(game, max_num_iterations, target_exploitability, print_progress, &SolveParams::current())
}

/// Proceeds one iteration with `SolveParams::current()`. See `solve_step_with_params`.
#[inline]
pub fn solve_step<T: Game>(game: &T, current_iteration: u32) {
    solve_step_with_params(game, current_iteration, &SolveParams::current())
}
```

Keep the existing doc comments on the `_with_params` functions and add the sentence "`params` selects the discounting; `SolveParams::current()` is the engine's historical behaviour."

- [ ] **Step 4: Run the unit tests and the oracle**

`cargo test --release --lib solver::tests` Expected: 3 passed.
`RUSTFLAGS="--deny warnings" cargo test --release --features zstd --test golden` Expected: 9 passed, `git status --short tests/golden/` empty.

- [ ] **Step 5: Commit**

```bash
git add src/solver.rs
git commit -m "feat: ✨ SolveParams with bit-exact current preset and paper preset"
```

### Task 4: Golden scenario and convergence tests for the paper preset

**Files:**
- Modify: `tests/golden.rs` (`solve_and_digest` and `check_golden` gain a `params: &SolveParams` argument; all existing call sites pass `&SolveParams::current()`).
- Create: `tests/golden/flop_paper_preset.txt`
- Modify: `tests/kuhn.rs:245-268`, `tests/leduc.rs` (the analogous test function).
- Modify: `docs/benchmarks.md` (scenario table gains the row).

- [ ] **Step 1: Thread `params` through the golden harness**

Change the signatures to:

```rust
fn check_golden(name: &str, mut game: PostFlopGame, compressed: bool, iterations: u32, lock: Option<fn(&mut PostFlopGame)>, params: &SolveParams)
fn solve_and_digest(name: &str, game: &mut PostFlopGame, compressed: bool, iterations: u32, lock: Option<fn(&mut PostFlopGame)>, params: &SolveParams) -> String
```

and inside `solve_and_digest` replace `solve(game, iterations, 0.0, false)` with `solve_with_params(game, iterations, 0.0, false, params)`. Update every existing test and the thread-independence test to pass `&SolveParams::current()`.

- [ ] **Step 2: Run the existing goldens**

`RUSTFLAGS="--deny warnings" cargo test --release --features zstd --test golden` Expected: 9 passed, no golden file modified.

- [ ] **Step 3: Add the new scenario (fails first)**

```rust
#[test]
fn flop_paper_preset() {
    let game = game(card_config("Td9d6h", None, None), tree_config(BoardState::Flop, 0.0, 0.0));
    check_golden("flop_paper_preset", game, true, 30, None, &SolveParams::paper());
}
```

Run `cargo test --release --features zstd --test golden flop_paper_preset` Expected: FAIL, golden file missing, message names the path.

- [ ] **Step 4: Create the golden and confirm stability**

`UPDATE_GOLDEN=1 cargo test --release --features zstd --test golden flop_paper_preset` then `cargo test --release --features zstd --test golden` twice. Expected: 10 passed both times; `git status --short tests/golden/` shows only the new file. Confirm the new digest differs from `flop_compressed.txt` (the two presets must not coincide): `diff -q tests/golden/flop_compressed.txt tests/golden/flop_paper_preset.txt` reports a difference.

- [ ] **Step 5: Kuhn and Leduc under both presets**

In `tests/kuhn.rs`, rename the body of `fn kuhn()` into `fn solve_kuhn(params: &SolveParams)` taking the params and calling `solve_with_params(&mut game, 10000, target, false, params)`, and add:

```rust
#[test]
fn kuhn() { solve_kuhn(&SolveParams::current()); }

#[test]
fn kuhn_paper_preset() { solve_kuhn(&SolveParams::paper()); }
```

Do the same in `tests/leduc.rs` for its solve test. Run `cargo test --release --features zstd --test kuhn --test leduc` Expected: all pass. If the paper preset misses the convergence bound, report the numbers; do not loosen the bound.

- [ ] **Step 6: Docs and gates**

Add the `flop_paper_preset` row to the scenario table in `docs/benchmarks.md` and one sentence on `SolveParams` under the golden section. Run the full gate list from Global Constraints.

- [ ] **Step 7: Commit**

```bash
git add tests/golden.rs tests/golden/flop_paper_preset.txt tests/kuhn.rs tests/leduc.rs docs/benchmarks.md
git commit -m "test: ✅ golden and convergence coverage for the paper preset"
```

### Task 5: Protocol 0.2 discount block in the adapter (hand-ranger, branch `protocol-0.2`, after the repoint)

**Files:**
- Modify: `adapters/postflop-solver/src/main.rs` (`SolveSettings`, `SolveResult`, the solve loop at lines 365-385, protocol version check).
- Modify: `adapters/postflop-solver/PROTOCOL.md` (new version section).
- Modify: `adapters/postflop-solver/Cargo.toml` (pin already repointed by Kevin's ADR step; verify `rev` is the fork revision that passed the gate).

**Interfaces:**
- Consumes: `SolveParams`, `RestartSchedule`, `solve_step_with_params` from Task 3.
- Produces: request `solve.discount { "alpha": 1.5, "beta": 0.0, "gamma": 3, "restart": "powers_of_four" | "none" | [100, 300] }`, optional; export `solve_result.discount` echoing the effective values (present only when the request carried the block, so 0.1 exports do not change).

- [ ] **Step 1: Failing test**

```rust
#[cfg(test)]
mod discount_tests {
    use super::*;

    #[test]
    fn absent_block_means_current_preset() {
        let s: SolveSettings = serde_json::from_str(r#"{"max_iterations":1,"target_exploitability_fraction_of_pot":0,"check_every":1,"compression":true}"#).unwrap();
        assert_eq!(s.params(), SolveParams::current());
        assert!(s.discount.is_none());
    }

    #[test]
    fn block_maps_to_params() {
        let s: SolveSettings = serde_json::from_str(r#"{"max_iterations":1,"target_exploitability_fraction_of_pot":0,"check_every":1,"compression":true,"discount":{"alpha":1.5,"beta":0.0,"gamma":2,"restart":"none"}}"#).unwrap();
        assert_eq!(s.params(), SolveParams::paper());
        let s: SolveSettings = serde_json::from_str(r#"{"max_iterations":1,"target_exploitability_fraction_of_pot":0,"check_every":1,"compression":true,"discount":{"alpha":1.5,"beta":0.0,"gamma":3,"restart":[100,300]}}"#).unwrap();
        assert_eq!(s.params().restart, RestartSchedule::At(vec![100, 300]));
    }
}
```

- [ ] **Step 2: Run to verify failure** `cargo test discount_tests` Expected: compile error.

- [ ] **Step 3: Implement**

```rust
#[derive(Deserialize, Serialize, Clone, PartialEq, Debug)]
#[serde(untagged)]
enum RestartSpec {
    Named(String),
    At(Vec<u32>),
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Debug)]
struct Discount {
    alpha: f64,
    beta: f64,
    gamma: u32,
    restart: RestartSpec,
}

impl SolveSettings {
    fn params(&self) -> SolveParams {
        match &self.discount {
            None => SolveParams::current(),
            Some(d) => SolveParams {
                alpha: d.alpha,
                beta: d.beta,
                gamma: d.gamma,
                restart: match &d.restart {
                    RestartSpec::Named(s) if s == "powers_of_four" => RestartSchedule::PowersOfFour,
                    RestartSpec::Named(s) if s == "none" => RestartSchedule::None,
                    RestartSpec::Named(s) => fail(format!("solve.discount.restart: unknown schedule `{s}`")),
                    RestartSpec::At(points) => RestartSchedule::At(points.clone()),
                },
            },
        }
    }
}
```

Add `#[serde(default)] discount: Option<Discount>` to `SolveSettings`; add `#[serde(skip_serializing_if = "Option::is_none")] discount: Option<Discount>` to `SolveResult` filled with `req.solve.discount.clone()`; accept `PROTOCOL_VERSION` 0.1 or 0.2 and echo the request's version in the export; in the solve loop compute `let params = req.solve.params();` once and call `solve_step_with_params(&game, i, &params)`. A 0.1 request that carries a `discount` block is a refusal (exit 2).

- [ ] **Step 4: Byte-identical check for 0.1**

Build, then solve a small 0.1 request twice, once with the previous adapter binary (built from the commit before this task) and once with this one, `RAYON_NUM_THREADS=1`, and `cmp` the exports. Expected: identical. Record the command in the report.

- [ ] **Step 5: Document and commit**

Add the 0.2 section to `PROTOCOL.md` (request field, export echo, the refusal rule, presets named `current` and `paper` with their values).

```bash
git add adapters/postflop-solver
git commit -m "feat: ✨ protocol 0.2 solve.discount block mapped to SolveParams"
```

**Handoff to Kevin:** tree/0.2 with `solve.discount`, generator pass-through into the 0.2 request and into `hand.json`'s configuration.

### Task 6: The two paper-preset results on hr-0002 (hand-ranger, evidence only)

**Files:**
- Create: `docs/evidence/hand-02/paper-preset/README.md` plus the two exports and stability outputs it references (exports are evidence files; check the repo's evidence policy in `AGENTS.md` for size limits before committing them, otherwise store hashes and a path in object storage).

- [ ] **Step 1: Result (a), the ADR-0013 method**

From `dataset/hands/hr-0002/solver-request.json` make two 0.2 requests with `discount` set to the paper preset, `max_iterations` 5000, `check_every` 10, and `target_exploitability_fraction_of_pot` 0.005 and 0.0025. Solve both at 4 threads. Grade one against the other with `compare.py` (per-step total variation). Report iterations run, exploitability, and the per-step table.

- [ ] **Step 2: Result (b), information only**

Grade the 0.005 export against the committed references with `generate --reuse-export` into a temp directory plus `compare.py`. Report the table and state explicitly that disagreement here is not a failure.

- [ ] **Step 3: Write the README and commit**

Sections: question, method, result (a) table, result (b) table, recommendation on whether fixed counts can go. Commit as `docs: 📝 paper preset stopping experiment on hr-0002`.

---

## Phase 1c: line derivation from saved games

### Task 7: Engine proof that a saved game reproduces a path (postflop-solver, branch `save-derive`)

**Files:**
- Create: `tests/save_derive.rs`
- Create: `docs/saved-games.md`

**Interfaces:**
- Consumes: `save_data_to_file`, `load_data_from_file`, `set_target_storage_mode`, `target_memory_usage` from `src/file.rs` and `src/game/serialization.rs`.
- Produces: the documented guarantee the adapter relies on: after `finalize`, saving at storage mode River and loading yields identical `strategy()`, `weights()`, `normalized_weights()` on every node of any path.

- [ ] **Step 1: Failing test**

```rust
use postflop_solver::*;
use std::path::PathBuf;

fn small_game() -> PostFlopGame {
    let card_config = CardConfig {
        range: ["66+,A8s+,AJo+,K9s+,KQo,QTs+".parse().unwrap(), "QQ-22,AQs-A2s,ATo+,K5s+,KJo+".parse().unwrap()],
        flop: flop_from_str("Js9s6d").unwrap(),
        turn: card_from_str("2c").unwrap(),
        river: NOT_DEALT,
    };
    let sizes: BetSizeOptions = ("75%", "2.5x").try_into().unwrap();
    let tree_config = TreeConfig {
        initial_state: BoardState::Turn,
        starting_pot: 550,
        effective_stack: 9750,
        turn_bet_sizes: [sizes.clone(), sizes.clone()],
        river_bet_sizes: [sizes.clone(), sizes],
        add_allin_threshold: 1.5,
        force_allin_threshold: 0.15,
        ..Default::default()
    };
    PostFlopGame::with_config(card_config, ActionTree::new(tree_config).unwrap()).unwrap()
}

/// Strategy and both players' weights at every node along `path`, in play order.
fn path_snapshot(game: &mut PostFlopGame, path: &[usize]) -> Vec<(Vec<f32>, Vec<f32>, Vec<f32>)> {
    game.back_to_root();
    let mut out = Vec::new();
    for &step in path {
        game.cache_normalized_weights();
        let strategy = if game.is_chance_node() || game.is_terminal_node() { vec![] } else { game.strategy() };
        out.push((strategy, game.weights(0).to_vec(), game.weights(1).to_vec()));
        game.play(step);
    }
    out
}

#[test]
fn saved_game_reproduces_a_path_bit_for_bit() {
    let mut game = small_game();
    game.allocate_memory(true);
    solve(&mut game, 20, 0.0, false);

    // OOP checks, IP bets, OOP calls, river card 0 (2d is index 1; pick the first dealable card), OOP checks.
    game.back_to_root();
    game.play(0); game.play(1); game.play(1);
    let river = (0..52).find(|&c| game.possible_cards() & (1u64 << c) != 0).unwrap();
    let path = vec![0, 1, 1, river, 0];
    let before = path_snapshot(&mut game, &path);

    let file: PathBuf = std::env::temp_dir().join(format!("save-derive-{}.bin", std::process::id()));
    save_data_to_file(&game, "test", &file, None).unwrap();
    let (mut loaded, memo): (PostFlopGame, String) = load_data_from_file(&file, None).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert_eq!(memo, "test");

    let after = path_snapshot(&mut loaded, &path);
    assert_eq!(before.len(), after.len());
    for (k, (b, a)) in before.iter().zip(&after).enumerate() {
        assert_eq!(b.0.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), a.0.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), "strategy at node {k}");
        assert_eq!(b.1.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), a.1.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), "oop weights at node {k}");
        assert_eq!(b.2.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), a.2.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), "ip weights at node {k}");
    }
}
```

The action indices assume the turn root offers check as action 0 and a bet as action 1 for IP; assert `game.available_actions()` at each step in the test so a wrong index fails loudly rather than silently walking a different line.

- [ ] **Step 2: Run** `cargo test --release --features zstd --test save_derive` Expected: fails if anything about save/load loses precision; if it passes on the first run, that is the result the task exists to establish. Either way record it.

- [ ] **Step 3: Measure what the file holds**

In the same test file add `#[test] fn saved_file_size_matches_target_memory_usage()`: after `finalize`, compare the saved file's byte length with `game.target_memory_usage()` plus the header written by `file.rs` (read `src/file.rs:1-10` for the header layout) and assert equality within the header size. Then read `src/game/serialization.rs` `num_target_storage` and `encode` and write `docs/saved-games.md`: what is saved at each `target_storage_mode` (River keeps every node's strategy; regrets are not saved; IP cfvalues are saved only if the mode keeps them), the formula for file size, that loading needs memory equal to `target_memory_usage`, and that a derived path from a loaded game is bit-identical to the solving process (this test).

- [ ] **Step 4: Gates and commit**

Full gate list. Then:

```bash
git add tests/save_derive.rs docs/saved-games.md
git commit -m "test: ✅ prove a saved game reproduces a path bit for bit"
```

### Task 8: `export.save_game` and `derive` mode in the adapter (hand-ranger, branch `protocol-0.2`)

**Files:**
- Modify: `adapters/postflop-solver/src/main.rs` (request `export` block, node walk extracted into `fn export_nodes`, `derive` subcommand).
- Modify: `adapters/postflop-solver/Cargo.toml` (add `sha2 = "0.10"`, and `postflop-solver` features must include `bincode`; the pin currently uses `default-features = false`, so add `features = ["bincode"]`).
- Modify: `adapters/postflop-solver/PROTOCOL.md`.

**Interfaces:**
- Consumes: Task 7's guarantee; `save_data_to_file(&game, memo, path, None)`, `load_data_from_file(path, None) -> (PostFlopGame, String)`.
- Produces:
  - Request (0.2 only): `"export": { "save_game": "/abs/or/relative/path.bin" }`, optional.
  - Memo string written into the saved file: JSON `{"solver_revision", "max_iterations", "iterations_run", "exploitability", "threads", "discount"}`.
  - CLI: `hand-ranger-postflop-adapter derive --saved FILE < request.json > export.json`.
  - Export additions in derive mode: `solve_result` copied from the memo; `derived_from { "saved_game_sha256", "saved_game_path", "solve": <memo>, "solver_revision" }`; `request_echo` is the derive request (same ranges, flop, tree; new path).

- [ ] **Step 1: Extract the node walk**

Move lines 400-470 (the loop that builds `nodes`) into `fn export_nodes(game: &mut PostFlopGame, path: &[PathStep]) -> (PlayerVectors<Vec<String>>, Vec<NodeExport>)` with no behaviour change. Rebuild and run the 0.1 byte-identical check from Task 5 Step 4. Expected: identical.

- [ ] **Step 2: Failing test for the memo round trip**

```rust
#[cfg(test)]
mod memo_tests {
    use super::*;
    #[test]
    fn memo_round_trips() {
        let m = SaveMemo { solver_revision: "abc".into(), max_iterations: 600, iterations_run: 600, exploitability: 0.57, threads: 4, discount: None };
        let s = serde_json::to_string(&m).unwrap();
        let back: SaveMemo = serde_json::from_str(&s).unwrap();
        assert_eq!(back.iterations_run, 600);
    }
}
```

- [ ] **Step 3: Implement save**

```rust
#[derive(Deserialize, Default)]
struct ExportSettings {
    #[serde(default)]
    save_game: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct SaveMemo {
    solver_revision: String,
    max_iterations: u32,
    iterations_run: u32,
    exploitability: f32,
    threads: usize,
    discount: Option<Discount>,
}
```

Add `#[serde(default)] export: ExportSettings` to `Request` (refuse it on 0.1 requests). After the export JSON has been written to stdout and flushed, if `save_game` is set: build the memo from `solve_result`, `save_data_to_file(&game, &serde_json::to_string(&memo).unwrap(), path, None)`, and print the path and byte length to stderr. Order matters and is documented: export first, save second.

- [ ] **Step 4: Implement derive**

In `main`, if `args[1] == "derive"`: parse `--saved FILE`; read the request from stdin (must be 0.2, must not set `export.save_game`); `let (mut game, memo_json) = load_data_from_file(file, None)`; parse the memo; verify the request against the loaded game: flop equals `game.card_config().flop`, both ranges equal (`Range` implements `PartialEq`; build the request's ranges the same way the solve path does), tree fields equal `game.tree_config()` field by field; any mismatch is a refusal listing the field. Compute the file's SHA-256 while streaming it once with `sha2`. Then `export_nodes(&mut game, &req.path)`, build `solve_result` from the memo (`max_iterations`, `iterations_run`, `exploitability`, `threads`, `reached_target: false`, `target_exploitability: 0.0`, `discount: memo.discount`, memory fields from `game.memory_usage()`), and set:

```rust
#[derive(Serialize)]
struct DerivedFrom {
    saved_game_path: String,
    saved_game_sha256: String,
    solve: SaveMemo,
    solver_revision: String,
}
```

as an `Option<DerivedFrom>` field on `Export` with `skip_serializing_if = "Option::is_none"`. `solver.revision` in a derived export is the running adapter's revision; `derived_from.solver_revision` is the memo's. Refuse if they differ.

- [ ] **Step 5: End-to-end check**

Solve the Task 2 smoke request with `export.save_game` set and 5 iterations; then `derive` with the same request (same path). `python3 -c` compare `nodes` of the two exports for equality of every float bit (`json.dumps` with the same options and `==`). Expected: equal. Then derive with a different legal path and confirm the export's `request_echo.path` is the new path and `derived_from` is present.

- [ ] **Step 6: Document and commit**

`PROTOCOL.md`: the `export` block, the memo, the `derive` subcommand, ordering (export before save), `derived_from`, refusals, the statement that loading needs memory equal to the file so rich-tree derivations run on the instance class that solved them, and that both solves of a hand need saved games for a derived line to pass the stability check.

```bash
git add adapters/postflop-solver
git commit -m "feat: ✨ save solved games and derive any line from them without re-solving"
```

**Handoff to Kevin:** generator acceptance of `derived_from` and of the copied `solve_result` counts; object storage location for saved games.

---

## Phase 2: cost model (prepared now; cloud runs gated on the AWS account and Terraform review)

### Task 9: Streaming-bandwidth bench (postflop-solver, branch `cost-model`)

**Files:**
- Create: `benches/bandwidth.rs`
- Modify: `Cargo.toml` (`[[bench]] name = "bandwidth" harness = false`)
- Modify: `docs/benchmarks.md` (section "Bandwidth")

- [ ] **Step 1: Write the bench**

```rust
//! Streaming memory bandwidth on this machine, single-threaded: the predictor for solve
//! iteration time, which is bandwidth bound.
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

const BYTES: usize = 1 << 30;

fn bandwidth(c: &mut Criterion) {
    let src = vec![1.0f32; BYTES / 4];
    let mut dst = vec![0.0f32; BYTES / 4];
    let mut group = c.benchmark_group("bandwidth");
    group.throughput(Throughput::Bytes(2 * BYTES as u64));
    group.sample_size(10);
    group.bench_function("copy_1gib", |b| {
        b.iter(|| {
            dst.copy_from_slice(black_box(&src));
            black_box(&dst);
        })
    });
    group.bench_function("scale_add_1gib", |b| {
        b.iter(|| {
            for (d, s) in dst.iter_mut().zip(&src) {
                *d = *d * 0.5 + *s;
            }
            black_box(&dst);
        })
    });
    group.finish();
}

criterion_group!(benches, bandwidth);
criterion_main!(benches);
```

- [ ] **Step 2: Run and record** `cargo bench --bench bandwidth` Expected: criterion prints GiB/s for both. Record in `docs/benchmarks.md` for the Apple M5 alongside the solve baselines, with the ratio seconds-per-iteration-per-GB of tree over bandwidth so the predictor has a constant.

- [ ] **Step 3: Gates and commit** Full gate list (the bench must compile under `--deny warnings`).

```bash
git add benches/bandwidth.rs Cargo.toml docs/benchmarks.md
git commit -m "perf: 📈 streaming bandwidth bench as the iteration-time predictor"
```

### Task 10: Cost runner and cloud runbook (hand-ranger, branch `cost-runner`)

**Files:**
- Create: `docs/research/cost/run.py`
- Create: `docs/research/cost/README.md`
- Create: `docs/research/cost/runbook-aws.md`
- Create: `docs/research/cost/instances.json`

**Interfaces:**
- Consumes: the adapter (any revision; the runner records `solver.revision` from a `plan_only` export), probe requests in `docs/research/tree-probe-200bb/`, `RAYON_NUM_THREADS`.
- Produces: `results/<host>-<date>.csv` with columns `request, threads, iterations, seconds_per_iteration, peak_rss_bytes, memory_bytes_compressed, solver_revision, host`; and `run.py cost --results FILE --instances instances.json` printing dollars per accepted hand per instance.

- [ ] **Step 1: Write the runner**

```python
#!/usr/bin/env python3
"""Measure seconds per iteration and peak memory for probe requests, then price a hand.

    run.py measure --adapter BIN --request FILE [--threads 1,2,4,8] [--iterations 30] --out results.csv
    run.py cost --results results.csv --instances instances.json [--hand-iterations 1600]
"""
import argparse, csv, json, os, platform, re, resource, subprocess, sys, tempfile, time

def plan(adapter, request):
    req = json.load(open(request)); req["solve"]["plan_only"] = True
    p = subprocess.run([adapter], input=json.dumps(req), capture_output=True, text=True)
    m = re.search(r"compressed ([0-9.]+) GB", p.stderr)
    return float(m.group(1)) * 1e9

def measure(args):
    req = json.load(open(args.request)); req["solve"]["plan_only"] = False
    req["solve"]["max_iterations"] = args.iterations; req["solve"]["check_every"] = args.iterations
    mem = plan(args.adapter, args.request)
    rev = None
    rows = []
    for t in [int(x) for x in args.threads.split(",")]:
        env = dict(os.environ, RAYON_NUM_THREADS=str(t))
        start = time.monotonic()
        p = subprocess.run([args.adapter], input=json.dumps(req), capture_output=True, text=True, env=env)
        wall = time.monotonic() - start
        if p.returncode != 0:
            sys.exit(f"adapter failed at {t} threads: {p.stderr[-2000:]}")
        export = json.loads(p.stdout); rev = export["solver"]["revision"]
        # Iteration lines on stderr are "iteration N: exploitability ..."; the first N is the setup cost.
        stamps = [float(x) for x in re.findall(r"^\[t=([0-9.]+)\] iteration", p.stderr, re.M)]
        secs = (stamps[-1] - stamps[0]) / (len(stamps) - 1) if len(stamps) > 1 else wall / args.iterations
        rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        rss = rss if platform.system() == "Darwin" else rss * 1024
        rows.append(dict(request=os.path.basename(args.request), threads=t, iterations=args.iterations,
                         seconds_per_iteration=round(secs, 3), peak_rss_bytes=rss, memory_bytes_compressed=int(mem),
                         solver_revision=rev, host=platform.node()))
        print(rows[-1], file=sys.stderr)
    new = not os.path.exists(args.out)
    with open(args.out, "a", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        if new: w.writeheader()
        w.writerows(rows)

def cost(args):
    rows = list(csv.DictReader(open(args.results)))
    instances = json.load(open(args.instances))
    print(f"{'instance':<14}{'$/h':>8}{'RAM GB':>8}{'request':<24}{'thr':>4}{'fit':>4}{'$/hand':>10}")
    for inst in instances:
        for r in rows:
            fit = max(1, int(inst["ram_gb"] * 1e9 // float(r["memory_bytes_compressed"])))
            secs = float(r["seconds_per_iteration"]) * inst.get("bandwidth_scale", 1.0)
            dollars = args.hand_iterations * secs * inst["usd_per_hour"] / 3600 / fit
            print(f"{inst['name']:<14}{inst['usd_per_hour']:>8.3f}{inst['ram_gb']:>8}{r['request']:<24}{r['threads']:>4}{fit:>4}{dollars:>10.2f}")

if __name__ == "__main__":
    ap = argparse.ArgumentParser(); sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("measure"); m.add_argument("--adapter", required=True); m.add_argument("--request", required=True)
    m.add_argument("--threads", default="1,2,4,8"); m.add_argument("--iterations", type=int, default=30); m.add_argument("--out", required=True)
    c = sub.add_parser("cost"); c.add_argument("--results", required=True); c.add_argument("--instances", required=True); c.add_argument("--hand-iterations", type=int, default=1600)
    a = ap.parse_args(); measure(a) if a.cmd == "measure" else cost(a)
```

The runner needs per-iteration timestamps on stderr. If the adapter's progress line does not carry one, add `[t=<seconds since start>]` to the adapter's iteration line in this task (stderr only, no export change) and note it in `PROTOCOL.md` under diagnostics.

- [ ] **Step 2: instances.json**

```json
[
  {"name": "r7a.4xlarge", "arch": "x86_64", "vcpu": 16, "ram_gb": 128, "usd_per_hour": 1.217, "bandwidth_scale": 1.0},
  {"name": "r8g.4xlarge", "arch": "arm64",  "vcpu": 16, "ram_gb": 128, "usd_per_hour": 0.943, "bandwidth_scale": 1.0}
]
```

Prices are the on-demand us-east-1 figures on the day the file is written; the README says to refresh them and that `bandwidth_scale` is the measured ratio of the instance's `bandwidth` bench to the Mac's, filled in after the cloud runs.

- [ ] **Step 3: Mac measurements**

Run `measure` on `j96-200-tree01.json` and `k72-200bb-tree01.json` at 1,2,4,8 threads, 30 iterations, on the M5 with nothing else running. Run `cost`. Put both tables in `README.md`.

- [ ] **Step 4: Runbook**

`runbook-aws.md`: prerequisites (account, the Terraform from the review, the solve image for both architectures), the exact sequence for one instance (launch, copy request, run `measure` at 4 and 16 threads and the `bandwidth` bench, copy results back, terminate), the same for the second architecture, then `cost` with `bandwidth_scale` filled in, and the decision rule for the default instance type in Terraform.

- [ ] **Step 5: Commit**

```bash
git add docs/research/cost adapters/postflop-solver/src/main.rs adapters/postflop-solver/PROTOCOL.md
git commit -m "feat: ✨ cost runner, instance table, and AWS runbook for solve pricing"
```

---

## Phase 3 (deferred)

Memory reductions (solve-only mode without the IP counterfactual buffer, dead-hand elimination with the node storage refactor, split precision) get their own plan after Task 10's numbers exist. Each will be gated by Task 2's script and priced by Task 10's runner.

## Self-review

- Spec 1a: Tasks 1, 2 (gate, build variables, three-condition pass rule, revision check, local run, ADR handoff).
- Spec 1b: Tasks 3, 4, 5, 6 (params, presets bit-exact, goldens, Kuhn and Leduc, protocol 0.2, tree/0.2 handoff, the two results).
- Spec 1c: Tasks 7, 8 (bit-exact save and load proof, `save_game`, `derive`, provenance, ordering, memory note, object storage note).
- Spec phase 2: Tasks 9, 10 (bandwidth predictor, runner, model, instance comparison, runbook).
- Spec phase 3: deferred by design, stated.
- Names used across tasks: `SolveParams`, `RestartSchedule`, `solve_with_params`, `solve_step_with_params` (Tasks 3, 4, 5); `Discount`, `SaveMemo`, `DerivedFrom`, `export_nodes` (Tasks 5, 8); `gate.sh` flags (Tasks 2, 8 Step 5, 10).
