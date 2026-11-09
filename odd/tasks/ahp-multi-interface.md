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
- [ ] T2 CLI `ahp` with `classic`/`shortII`, `--no-verbose`, interactive questionnaire
      (route: delegated writer)
- [ ] T3 Slint UI: shell, start screen (mode, load/save file), classic flow (4 screens)
      (route: delegated writer)
- [ ] T4 Slint UI: AHPSort II flow (5 screens, local priority tables + charts)
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
- T1: delegated writer; `cargo test --workspace` → 41 passed (parent spot check); bugs 1-4 fixed with regression tests; `ahp::project` API + 4 example files. Range evaluation order = evaluate_ranges order (values DESC, RP6..RP1).

## Next step

T2 (CLI).
