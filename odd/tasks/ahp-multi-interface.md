# Feature: ahp-multi-interface

## Objective

Split the project into a Cargo workspace with three pieces:

- `ahp` (library): AHP logic (classic and AHPSort II), the `.ahp` file format and a
  high-level "solve" API shared by both interfaces.
- `cli` (binary `ahp`): `ahp classic <file>` / `ahp shortII <file>`, `--no-verbose`.
- `ui` (binary `ahp-ui`): Slint desktop app with the guided screens for both methods.

## Problem / why

The library only exposes low-level building blocks and has a few bugs that block real
use (constructor validation swapped, profiles assigned to categories in reverse order,
limiting-profile count check wrong, range priorities not refreshed after editing a cell).
Users need a file format to load/save problems and two front-ends.

## Constraints

- Generated artifacts (code, comments, UI copy) in English by default **except UI copy and
  CLI user-facing text, which the user specified in Spanish** (labels like "Mucho mejor",
  "Saltar", "Continuar", "Siguiente", "Detalles prioridad local").
- Values table: in code each row is a criterion, columns are alternatives
  (`Table = Vec<(unit, Vec<f64>)>`). The UI shows alternatives as rows.
- AHPSort II range: only the maximum matters; RP points always go from 0 to max
  (`step = max / 5`). Default max = max of the criterion row (0 if empty) unless the user
  edited it manually (manual flag).
- Saaty enum is 2..9; sign is encoded by `AhpScore::{LeftDominates, Equal, RightDominates}`.
- Weight method: `WeightMethod::EigenValues` everywhere.
- Slint 1.18.1, clap 4.6.x, serde 1.0.x, toml (0.8 or 1.x) are in the local cargo cache.

## Design contract (shared by all tasks)

### `.ahp` file (TOML)

```toml
method = "shortII"               # optional hint: "classic" | "shortII"
alternatives = ["Barking", "Barnet", "Camden"]

[[criteria]]
name = "Crímenes"
unit = "delitos"
values = [12.0, 30.0, 80.0]      # one value per alternative
range = 206.0                    # optional (AHPSort II): max of the range; absent => auto (max of values)
profiles = [20.0, 100.0, 160.0]  # AHPSort II: one value per category that has a profile

[categories]                     # AHPSort II only
profile_type = "central"         # "central" | "limiting"
names = ["Alto", "Medio", "Bajo"]
# central: one profile per category; limiting: one profile per category except the last one,
# which has no profile and collects every alternative above all limits.

[evaluations]                    # optional, any part may be missing
criteria = [3, -5, 1]            # signed Saaty, quiz order (0,1),(0,2),...,(1,2),...
[evaluations.alternatives]       # classic: keyed by criterion name, quiz order of alternatives
"Crímenes" = [1, 2, -3]
[evaluations.ranges]             # AHPSort II: keyed by criterion name, quiz order of range points
"Crímenes" = [ ... ]
```

Signed value `x`: `|x| >= 1`. `x >= 1` => left dominates, matrix cell `a_ij = x`;
`x <= -1` => right dominates, `a_ij = 1/|x|`; `1`/`-1` => equal. Floats allowed (lossless
for matrix edits); a quiz shows the nearest Saaty integer.

### Library API additions (task T1)

- `share::quiz`: `AhpScore::from_signed(f64)`, `AhpScore::to_signed()`, `signed_to_ratio`,
  `ratio_to_signed`, `QuizPairMatrix::labels()`, Spanish label helper for the 17 points.
- `project` module: `Project` (serde), `load/save/from_toml_str/to_toml_string`,
  `effective_range`, validation, `classic_quizzes`, `short_quizzes`,
  `solve_classic`, `solve_short` returning plain view structs (matrix labels + cells +
  `WeightsReport`, ranking sorted descending, local priority points, alternative/profile
  local+global priorities, classification).

## Tasks

- [x] T0 Explore library and environment (route: inline, ~1.3k lines read once)
- [x] T1 Workspace restructure + library fixes + `.ahp` format + solve API + examples + tests
      (route: delegated writer — 2+ non-trivial files)
- [x] T2 CLI `ahp` with `classic`/`shortII`, `--no-verbose`, interactive questionnaire
      (route: delegated writer)
- [x] T3 Slint UI: shell, start screen (mode, load/save file), classic flow (4 screens)
      (route: delegated writer)
- [x] T5 Dark mode: Theme light/dark tokens, starts from the system scheme, top-bar toggle (route: inline, mechanical token edits)
- [x] T4 Slint UI: AHPSort II flow (5 screens, local priority tables + charts)
      (route: delegated writer)

## Acceptance criteria

- `cargo build --workspace` and `cargo test --workspace` pass.
- `ahp classic examples/<file>.ahp` and `ahp shortII examples/<file>.ahp` print results;
  missing evaluations are asked interactively; `--no-verbose` prints only the result.
- UI: forward/back navigation everywhere, "Saltar" from questionnaire to matrices,
  editable off-diagonal matrix cells with live weights, file load + "skip to result".

## Checks

- `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace` (advisory).
- CLI smoke run on the examples (non-interactive and piped stdin).
- UI: build + launch smoke (headless where possible).

## Delivery

- Strategy: `ask-on-risk`. Forecast well above 400 authored lines (UI alone ~2k).
- Base boundary for review: `3d14814`.
- Engram mirror: **pending** (engram tools unavailable in this session).

## Progress / evidence

- Baseline: `cargo test -p ahp` → 15 passed.
- T1: delegated writer; `cargo test --workspace` → 41 passed (parent spot check); bugs 1-4 fixed with regression tests; `ahp::project` API + 4 example files. Range evaluation order = evaluate_ranges order (values DESC, RP6..RP1). Commit `15e75f7`.
- T1 review: risk medium, consent granted, lens reliability → approved and acknowledged (lineage review-9064802aed7a4758). Reviewed boundary advances to `15e75f7`.
  Advisory follow-ups (non-blocking): R3-001 solve_* accept non-finite/|x|<1 evaluation values (validate values in check_evaluations — scheduled with T3, the UI edits matrices); R3-002/R3-004 limiting mode semantics only enforced at project layer; R3-003 London example test should assert exact classes; R3-005 set_matrix_range_value always uses EigenValues.
- T2: delegated writer; `cargo test --workspace` → 41 + 15 + 13 passed (parent spot check); smoke `ahp classic examples/coches.ahp --no-verbose` OK.
  Note: limiting-profile convention in the library = profile k is the UPPER limit of category k and the profile-less LAST category collects everything above (categories declared worst→best). The UI must explain this.
- T2 commit `7e7e77e`. T2 review: medium, granted, lens reliability → approved + acknowledged (lineage review-dfdcd27cb495aae3). Advisory R3-001 (save before solve) fixed in a follow-up commit; R3-002 (weak mismatch test) and R3-003 (unwrap_or on missing list) left as follow-ups.
- T3: delegated writer (first one stopped by accident, resumed by a fresh writer that kept its work); `cargo test --workspace` → 42 + 15 + 13 + 21 passed (parent spot check); clippy clean in ui; fmt clean; launch smoke exit 124 (window up). R3-001 lib follow-up fixed (check_signed in check_evaluations).
- T3 commit `60131d7`. T3 review: medium, granted, lens reliability → approved + acknowledged (lineage review-ca5b87f5a066b819). Advisory follow-ups: R3-001 rejected matrix cell can show as valid after another edit (ui/src/models.rs update_matrix); R3-002 save ignores invalid data cells (writes previous value silently); R3-003 missing CLI test for save-after-solve.
- T4: delegated writer; `cargo test --workspace` → 42 + 15 + 13 + 41 passed (parent spot check); clippy clean in ui; fmt clean; smoke exit 124 with londres.ahp. Includes T3 review fixes R3-001 (rejected matrix cell restored) and R3-002 (save refused while cells are invalid). Extra correctness fix: range answers remapped to their pairs when range points reorder.
- T4 commit `14b3b58`. T4 review: medium, granted, lens reliability → approved + acknowledged (lineage review-513c20f83231e159). Advisory follow-ups: R3-001 can_jump_to_result computed before profile padding on load; R3-002 rename_category accepts empty/duplicate names (classification looks up by name); R3-003 ProfilesStore in-place update paths untested.
  R3-001 fixed (load disables 'Ir al resultado' when reshaping resets answers) and R3-002 covered by tests (library validation already blocks empty/duplicate category names before solving) in a follow-up commit; ahp-ui tests 41 → 44.
- Delivery strategy: ask-on-risk → user chose `feature-branch-chain`.

## Next step

Feature complete. Pending human decisions: push / PR (feature-branch-chain). Open follow-ups: CLI R3-002/R3-003 (T2), lib R3-002..R3-005 (T1), T3 R3-003 (CLI save-after-solve test).
