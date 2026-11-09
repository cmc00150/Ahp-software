//! Conversion of [`AppState`] into the Slint stores.
//!
//! `refresh` rebuilds the models of the current screen (used after navigation, loading and
//! structure changes). The `update_*` functions patch a single row/cell in place so inputs
//! keep their focus while the user edits.

use std::rc::Rc;

use ahp::{MatrixView, Method, ProfileType, QuizItem, Value, quizzes};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{
    AppWindow, ChartTick, ClassRow, ClassificationStore, DataCell, DataColumn, DataRow, DataStore,
    LocalRow, LocalStore, LocalTable, MatrixCardData, MatrixCell, MatrixStats, MatrixStore,
    NavStore, PrioritiesStore, ProfileColumn, ProfileRow, ProfilesStore, QuizRow, QuizStore,
    RangeCell, RankItem, RankingStore, ResultRow,
    chart::{self, Tick},
    convert::{
        arrow_direction, fmt_ratio, fmt_value, fmt_weight, position_label, rank_scale,
        signed_to_position,
    },
    state::{AppState, Screen},
};

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(items)))
}

fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    model(items.into_iter().map(SharedString::from).collect())
}

/// Rebuilds the shell and the models of the current screen.
pub fn refresh(ui: &AppWindow, st: &AppState) {
    refresh_nav(ui, st);
    match st.screen {
        Screen::Start => {}
        Screen::Data => {
            refresh_data(ui, st);
            if st.method == Method::sortII {
                refresh_profiles(ui, st);
            }
        }
        Screen::Quiz => refresh_quiz(ui, st),
        Screen::Matrices => {
            refresh_matrices(ui, st);
            if st.method == Method::sortII {
                refresh_local(ui, st);
            }
        }
        Screen::Ranking => refresh_ranking(ui, st),
        Screen::Priorities => refresh_priorities(ui, st),
        Screen::Classification => refresh_classification(ui, st),
    }
}

pub fn refresh_nav(ui: &AppWindow, st: &AppState) {
    let nav = ui.global::<NavStore>();
    nav.set_screen(st.screen.index());
    nav.set_max_reached(st.max_reached.index());
    nav.set_classic(st.method == Method::Classic);
    nav.set_has_project(st.has_project);
    let file = st
        .file
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    nav.set_file_name(file.into());
}

/// Shows a message in the global message bar.
pub fn show_message(ui: &AppWindow, text: &str, is_error: bool) {
    let nav = ui.global::<NavStore>();
    nav.set_message(text.into());
    nav.set_message_is_error(is_error);
}

// ---------------------------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------------------------

fn data_cell(st: &AppState, a: usize, c: usize) -> DataCell {
    match st.invalid_cells.get(&(a, c)) {
        Some(text) => DataCell {
            text: text.into(),
            valid: false,
        },
        None => DataCell {
            text: fmt_value(st.project.criteria[c].values[a]).into(),
            valid: true,
        },
    }
}

pub fn refresh_data(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<DataStore>();
    let columns = st
        .project
        .criteria
        .iter()
        .map(|c| DataColumn {
            name: c.name.as_str().into(),
            unit: c.unit.as_str().into(),
        })
        .collect();
    let rows = st
        .project
        .alternatives
        .iter()
        .enumerate()
        .map(|(a, name)| DataRow {
            name: name.as_str().into(),
            cells: model(
                (0..st.project.criteria.len())
                    .map(|c| data_cell(st, a, c))
                    .collect(),
            ),
        })
        .collect();
    store.set_columns(model(columns));
    store.set_rows(model(rows));
    refresh_data_status(ui, st);
}

/// Inline errors and the "Ir al resultado" button.
pub fn refresh_data_status(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<DataStore>();
    store.set_errors(st.data_errors.as_str().into());
    store.set_can_jump_to_result(st.can_jump_to_result);
}

/// Updates the validity flag of one data cell without touching what the user is typing.
pub fn update_data_cell(ui: &AppWindow, st: &AppState, a: usize, c: usize) {
    let rows = ui.global::<DataStore>().get_rows();
    if let Some(row) = rows.row_data(a) {
        let mut cell = row.cells.row_data(c).unwrap_or_default();
        let valid = !st.invalid_cells.contains_key(&(a, c));
        if cell.valid != valid {
            cell.valid = valid;
            row.cells.set_row_data(c, cell);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Questionnaires
// ---------------------------------------------------------------------------------------------

fn detail(value: &Option<Value>) -> String {
    value
        .as_ref()
        .map(|v| {
            format!("{} {}", fmt_value(v.data), v.unit)
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

fn quiz_row(item: &QuizItem, signed: f64) -> QuizRow {
    let position = signed_to_position(signed);
    QuizRow {
        left_name: item.label1.as_str().into(),
        left_detail: detail(&item.ref_value1).into(),
        right_name: item.label2.as_str().into(),
        right_detail: detail(&item.ref_value2).into(),
        position,
        label: position_label(position).into(),
        direction: arrow_direction(position),
    }
}

fn criterion_title(st: &AppState, k: usize) -> String {
    let c = &st.project.criteria[k];
    if c.unit.trim().is_empty() {
        c.name.clone()
    } else {
        format!("{} ({})", c.name, c.unit)
    }
}

pub fn refresh_quiz(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<QuizStore>();
    let (criteria_quiz, per_criterion) = match quizzes(&st.project, st.method) {
        Ok(q) => q,
        Err(e) => {
            show_message(ui, &e.to_string(), true);
            return;
        }
    };
    let group = st.quiz_group;
    let quiz = match group {
        0 => &criteria_quiz,
        k => &per_criterion[k - 1].1,
    };
    let values = st.group_values(group).unwrap_or_default();
    let rows = quiz
        .scores()
        .iter()
        .enumerate()
        .map(|(i, item)| quiz_row(item, values.get(i).copied().unwrap_or(1.0)))
        .collect();

    let (title, subtitle) = if group == 0 {
        (
            "Importancia de los criterios".to_string(),
            "Para cada par, indica qué criterio es más importante y cuánto.".to_string(),
        )
    } else if st.method == Method::Classic {
        (
            format!("Criterio evaluado: {}", criterion_title(st, group - 1)),
            "Para cada par, indica qué alternativa es mejor según este criterio y cuánto."
                .to_string(),
        )
    } else {
        (
            format!("Rango del criterio: {}", criterion_title(st, group - 1)),
            "Para cada par de puntos del rango (RP1…RP6 y los perfiles de las categorías), \
             indica cuál es preferible según este criterio y cuánto."
                .to_string(),
        )
    };

    let steps = std::iter::once("Criterios".to_string()).chain(st.project.criteria_names());
    store.set_steps(strings(steps));
    store.set_current(group as i32);
    store.set_title(title.into());
    store.set_subtitle(subtitle.into());
    store.set_rows(model(rows));
}

/// Updates one question after the user picked a dot.
pub fn update_quiz_row(ui: &AppWindow, st: &AppState, item: usize) {
    let rows = ui.global::<QuizStore>().get_rows();
    let signed = st
        .group_values(st.quiz_group)
        .and_then(|v| v.get(item).copied());
    if let (Some(mut row), Some(signed)) = (rows.row_data(item), signed) {
        let position = signed_to_position(signed);
        row.position = position;
        row.label = position_label(position).into();
        row.direction = arrow_direction(position);
        rows.set_row_data(item, row);
    }
}

// ---------------------------------------------------------------------------------------------
// Matrices
// ---------------------------------------------------------------------------------------------

fn matrix_cells(view: &MatrixView) -> Vec<Vec<MatrixCell>> {
    view.cells
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.iter()
                .enumerate()
                .map(|(j, &v)| MatrixCell {
                    text: fmt_ratio(v).into(),
                    editable: i != j,
                    valid: true,
                })
                .collect()
        })
        .collect()
}

fn matrix_stats(view: &MatrixView) -> MatrixStats {
    let opt = |v: Option<f64>| v.map_or_else(|| "—".to_string(), fmt_weight);
    MatrixStats {
        weights: strings(view.report.weights.iter().map(|&w| fmt_weight(w))),
        lambda_max: opt(view.report.lambda_max).into(),
        ci: opt(view.report.ci).into(),
        cr: opt(view.report.cr).into(),
        inconsistent: view.report.cr.is_some_and(|cr| cr > 0.1),
    }
}

fn unit_subtitle(st: &AppState, k: usize) -> String {
    let unit = st.project.criteria[k].unit.trim();
    if unit.is_empty() {
        String::new()
    } else {
        format!("Unidad: {unit}")
    }
}

/// Matrices in card order: criteria first, then one per criterion (alternatives in classic
/// AHP, range points in AHPSort II).
fn matrix_views(st: &AppState) -> Vec<(String, String, &MatrixView)> {
    let (criteria, per_criterion, title): (_, Vec<&MatrixView>, fn(&str) -> String) =
        if let Some(o) = st.classic_outcome() {
            (
                &o.criteria,
                o.alternatives.iter().map(|(_, v)| v).collect(),
                |name| format!("Alternativas según «{name}»"),
            )
        } else if let Some(o) = st.sort_outcome() {
            (
                &o.criteria,
                o.ranges.iter().map(|(_, v)| v).collect(),
                |name| format!("Puntos del rango de «{name}»"),
            )
        } else {
            return Vec::new();
        };
    let mut cards = vec![(
        "Criterios".to_string(),
        "Importancia relativa de cada criterio".to_string(),
        criteria,
    )];
    for (k, view) in per_criterion.into_iter().enumerate() {
        cards.push((
            title(&st.project.criteria[k].name),
            unit_subtitle(st, k),
            view,
        ));
    }
    cards
}

fn card_data(title: &str, subtitle: &str, view: &MatrixView) -> MatrixCardData {
    MatrixCardData {
        title: title.into(),
        subtitle: subtitle.into(),
        labels: strings(view.labels.iter().cloned()),
        rows: model(matrix_cells(view).into_iter().map(model).collect()),
    }
}

pub fn refresh_matrices(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<MatrixStore>();
    let views = matrix_views(st);
    let cards = views
        .iter()
        .map(|(title, subtitle, view)| card_data(title, subtitle, view))
        .collect();
    let stats = views.iter().map(|(_, _, v)| matrix_stats(v)).collect();
    store.set_cards(model(cards));
    store.set_stats(model(stats));
}

/// Writes the cells of `view` into `data` in place, touching only the cells that differ.
/// A cell rejected before holds the rejected text, so it is always rewritten with the real
/// value (see [`reject_cell`]).
fn patch_card(data: &MatrixCardData, view: &MatrixView) {
    for (i, fresh_row) in matrix_cells(view).into_iter().enumerate() {
        let Some(row) = data.rows.row_data(i) else {
            continue;
        };
        for (j, fresh) in fresh_row.into_iter().enumerate() {
            if row.row_data(j).as_ref() != Some(&fresh) {
                row.set_row_data(j, fresh);
            }
        }
    }
}

/// Marks cell `(i, j)` as rejected and stores the rejected text in the model, so the model
/// matches what the input shows; the next rebuild then differs from it and puts the real
/// value (and a valid frame) back instead of leaving the rejected text looking valid.
fn reject_cell(data: &MatrixCardData, i: usize, j: usize, text: &str) {
    if let Some(row) = data.rows.row_data(i)
        && let Some(mut cell) = row.row_data(j)
    {
        cell.valid = false;
        cell.text = text.into();
        row.set_row_data(j, cell);
    }
}

/// Refreshes the cells and figures of one matrix in place (keeps the focused input).
pub fn update_matrix(ui: &AppWindow, st: &AppState, card: usize) {
    let store = ui.global::<MatrixStore>();
    let views = matrix_views(st);
    let (Some((_, _, view)), Some(data)) = (views.get(card), store.get_cards().row_data(card))
    else {
        return;
    };
    patch_card(&data, view);
    store.get_stats().set_row_data(card, matrix_stats(view));
}

/// Marks a matrix cell as invalid (the input keeps the text the user typed).
pub fn mark_matrix_cell_invalid(ui: &AppWindow, card: usize, i: usize, j: usize, text: &str) {
    if let Some(data) = ui.global::<MatrixStore>().get_cards().row_data(card) {
        reject_cell(&data, i, j, text);
    }
}

// ---------------------------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------------------------

pub fn refresh_ranking(ui: &AppWindow, st: &AppState) {
    let ranking = st
        .classic_outcome()
        .map(|o| o.ranking.as_slice())
        .unwrap_or_default();
    let top = ranking.first().map_or(1.0, |r| r.1).max(f64::EPSILON);
    let items = ranking
        .iter()
        .enumerate()
        .map(|(k, (name, score))| RankItem {
            position: k as i32 + 1,
            name: name.as_str().into(),
            score: format!("{} ({:.1} %)", fmt_weight(*score), score * 100.0).into(),
            share: (score / top).clamp(0.0, 1.0) as f32,
            scale: rank_scale(k),
        })
        .collect();
    ui.global::<RankingStore>().set_items(model(items));
}

// ---------------------------------------------------------------------------------------------
// AHPSort II: profiles table (data screen)
// ---------------------------------------------------------------------------------------------

fn profile_type(st: &AppState) -> ProfileType {
    st.categories()
        .map_or(ProfileType::Central, |c| c.profile_type)
}

/// Category columns: every category, the last one without profile in limiting mode.
pub fn profile_columns(st: &AppState) -> Vec<ProfileColumn> {
    let n_profiles = st.project.expected_profiles().unwrap_or(0);
    st.categories()
        .map(|c| c.names.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(k, name)| ProfileColumn {
            name: name.as_str().into(),
            has_profile: k < n_profiles,
        })
        .collect()
}

fn profile_cell(st: &AppState, c: usize, k: usize) -> DataCell {
    match st.invalid_profiles.get(&(c, k)) {
        Some(text) => DataCell {
            text: text.into(),
            valid: false,
        },
        None => DataCell {
            text: fmt_value(st.project.criteria[c].profiles[k]).into(),
            valid: true,
        },
    }
}

/// One row per criterion, in criterion order, with its profile values.
pub fn profile_rows(st: &AppState) -> Vec<ProfileRow> {
    st.project
        .criteria
        .iter()
        .enumerate()
        .map(|(c, criterion)| ProfileRow {
            name: criterion.name.as_str().into(),
            unit: criterion.unit.as_str().into(),
            cells: model(
                (0..criterion.profiles.len())
                    .map(|k| profile_cell(st, c, k))
                    .collect(),
            ),
        })
        .collect()
}

/// Range maximum of criterion `c`: the rejected text, or the effective maximum.
pub fn range_cell(st: &AppState, c: usize) -> RangeCell {
    match st.invalid_ranges.get(&c) {
        Some(text) => RangeCell {
            text: text.into(),
            valid: false,
            manual: true,
        },
        None => RangeCell {
            text: fmt_value(st.project.effective_range(c)).into(),
            valid: true,
            manual: st.range_is_manual(c),
        },
    }
}

fn catch_all(st: &AppState) -> String {
    match st.categories() {
        Some(c) if c.profile_type == ProfileType::Limiting => {
            c.names.last().cloned().unwrap_or_default()
        }
        _ => String::new(),
    }
}

pub fn refresh_profiles(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<ProfilesStore>();
    store.set_limiting(profile_type(st) == ProfileType::Limiting);
    store.set_columns(model(profile_columns(st)));
    store.set_rows(model(profile_rows(st)));
    store.set_ranges(model(
        (0..st.project.criteria.len())
            .map(|c| range_cell(st, c))
            .collect(),
    ));
    store.set_catch_all(catch_all(st).into());
}

/// Name of the profiles row of criterion `c` after a rename in the data table.
pub fn update_profile_row_name(ui: &AppWindow, st: &AppState, c: usize) {
    let rows = ui.global::<ProfilesStore>().get_rows();
    if let (Some(mut row), Some(criterion)) = (rows.row_data(c), st.project.criteria.get(c)) {
        row.name = criterion.name.as_str().into();
        row.unit = criterion.unit.as_str().into();
        rows.set_row_data(c, row);
    }
}

/// Range cell of criterion `c`, in place. `typed` keeps the text the user is typing (it only
/// updates the flags); otherwise the text is the current maximum.
pub fn update_range(ui: &AppWindow, st: &AppState, c: usize, typed: Option<&str>) {
    let ranges = ui.global::<ProfilesStore>().get_ranges();
    let mut cell = range_cell(st, c);
    if let Some(text) = typed {
        cell.text = text.into();
    }
    if ranges.row_data(c).as_ref() != Some(&cell) {
        ranges.set_row_data(c, cell);
    }
}

/// Validity flag of profile `(c, k)`, without touching the text being typed.
pub fn update_profile_cell(ui: &AppWindow, st: &AppState, c: usize, k: usize) {
    let rows = ui.global::<ProfilesStore>().get_rows();
    if let Some(row) = rows.row_data(c) {
        let mut cell = row.cells.row_data(k).unwrap_or_default();
        let valid = !st.invalid_profiles.contains_key(&(c, k));
        if cell.valid != valid {
            cell.valid = valid;
            row.cells.set_row_data(k, cell);
        }
    }
}

/// Category name of a header after a rename (the explanatory note names the last one).
pub fn update_category_name(ui: &AppWindow, st: &AppState) {
    ui.global::<ProfilesStore>()
        .set_catch_all(catch_all(st).into());
}

// ---------------------------------------------------------------------------------------------
// AHPSort II: local priorities (matrices screen)
// ---------------------------------------------------------------------------------------------

/// sort label of profile `k`: `CP1`, `CP2`... (central) or `LP1`... (limiting).
fn profile_label(st: &AppState, k: usize) -> String {
    match profile_type(st) {
        ProfileType::Central => format!("CP{}", k + 1),
        ProfileType::Limiting => format!("LP{}", k + 1),
    }
}

fn ticks(ticks: Vec<Tick>) -> ModelRc<ChartTick> {
    model(
        ticks
            .into_iter()
            .map(|t| ChartTick {
                pos: t.pos as f32,
                label: t.label.into(),
            })
            .collect(),
    )
}

/// One table per criterion (points ascending by value) with its chart.
pub fn local_tables(st: &AppState) -> Vec<LocalTable> {
    let Some(outcome) = st.sort_outcome() else {
        return Vec::new();
    };
    outcome
        .local_priorities
        .iter()
        .enumerate()
        .map(|(c, (name, points))| {
            let rows = points
                .iter()
                .map(|p| LocalRow {
                    label: p
                        .profile
                        .map_or_else(|| p.label.clone(), |k| profile_label(st, k))
                        .into(),
                    x: fmt_value(p.original).into(),
                    y: fmt_weight(p.weight).into(),
                })
                .collect();
            let xy: Vec<_> = points.iter().map(|p| (p.original, p.weight)).collect();
            let geometry = chart::chart(&xy);
            LocalTable {
                title: name.as_str().into(),
                unit: st
                    .project
                    .criteria
                    .get(c)
                    .map_or("", |c| c.unit.as_str())
                    .into(),
                rows: model(rows),
                plot_width: geometry.width as f32,
                plot_height: geometry.height as f32,
                line: geometry.line.into(),
                markers: geometry.markers.into(),
                x_ticks: ticks(geometry.x_ticks),
                y_ticks: ticks(geometry.y_ticks),
            }
        })
        .collect()
}

/// `CP1 = Alto · CP2 = Medio · ...`
fn local_legend(st: &AppState) -> String {
    let n = st.project.expected_profiles().unwrap_or(0);
    st.categories()
        .map(|c| c.names.as_slice())
        .unwrap_or_default()
        .iter()
        .take(n)
        .enumerate()
        .map(|(k, name)| format!("{} = {name}", profile_label(st, k)))
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn refresh_local(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<LocalStore>();
    store.set_tables(model(local_tables(st)));
    store.set_legend(local_legend(st).into());
}

// ---------------------------------------------------------------------------------------------
// AHPSort II: global priorities and classification
// ---------------------------------------------------------------------------------------------

fn result_rows(rows: &[(String, Vec<f64>, f64)]) -> Vec<ResultRow> {
    rows.iter()
        .map(|(name, local, global)| ResultRow {
            name: name.as_str().into(),
            cells: strings(local.iter().map(|&p| fmt_weight(p))),
            total: fmt_weight(*global).into(),
        })
        .collect()
}

pub fn refresh_priorities(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<PrioritiesStore>();
    let (alternatives, profiles) = st
        .sort_outcome()
        .map(|o| (result_rows(&o.alternatives), result_rows(&o.profiles)))
        .unwrap_or_default();
    store.set_criteria(strings(st.project.criteria_names()));
    store.set_alternatives(model(alternatives));
    store.set_profiles(model(profiles));
}

/// One row per alternative: its global priority, the global priority of the profile of its
/// category (`—` when that category has no profile) and the category.
pub fn classification_rows(st: &AppState) -> Vec<ClassRow> {
    let Some(outcome) = st.sort_outcome() else {
        return Vec::new();
    };
    let names = st
        .categories()
        .map(|c| c.names.as_slice())
        .unwrap_or_default();
    outcome
        .classification
        .iter()
        .map(|(name, global, category)| ClassRow {
            name: name.as_str().into(),
            global: fmt_weight(*global).into(),
            profile_global: outcome
                .profiles
                .iter()
                .find(|(label, _, _)| label == category)
                .map_or_else(|| "—".to_string(), |(_, _, g)| fmt_weight(*g))
                .into(),
            category: category.as_str().into(),
            category_index: names
                .iter()
                .position(|n| n == category)
                .map_or(-1, |k| k as i32),
        })
        .collect()
}

pub fn refresh_classification(ui: &AppWindow, st: &AppState) {
    ui.global::<ClassificationStore>()
        .set_rows(model(classification_rows(st)));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn loaded(name: &str) -> AppState {
        let mut st = AppState::new();
        let path = PathBuf::from(format!("{}/../examples/{name}", env!("CARGO_MANIFEST_DIR")));
        assert_eq!(st.load_file(&path).unwrap(), None);
        st
    }

    fn cell(data: &MatrixCardData, i: usize, j: usize) -> MatrixCell {
        data.rows.row_data(i).unwrap().row_data(j).unwrap()
    }

    #[test]
    fn a_rejected_matrix_cell_gets_its_value_back_on_the_next_rebuild() {
        let mut st = loaded("coches.ahp");
        st.jump_to_result().unwrap();
        let view = st.classic_outcome().unwrap().criteria.clone();
        let data = card_data("Criterios", "", &view);

        reject_cell(&data, 0, 1, "abc");
        assert_eq!(cell(&data, 0, 1).text, "abc");
        assert!(!cell(&data, 0, 1).valid);

        // Another cell of the same card is edited successfully: the rebuild restores the
        // stored value of the rejected one, with a valid frame.
        st.edit_matrix(0, 0, 2, "4").unwrap();
        let view = st.classic_outcome().unwrap().criteria.clone();
        patch_card(&data, &view);
        assert_eq!(cell(&data, 0, 1).text, fmt_ratio(view.cells[0][1]));
        assert!(cell(&data, 0, 1).valid);
        assert_eq!(cell(&data, 0, 2).text, "4");
    }

    #[test]
    fn profiles_table_follows_the_criteria() {
        let mut st = loaded("londres.ahp");
        let names = |st: &AppState| -> Vec<String> {
            profile_rows(st)
                .iter()
                .map(|r| r.name.to_string())
                .collect()
        };
        assert_eq!(names(&st), st.project.criteria_names());
        assert_eq!(profile_rows(&st)[0].cells.row_count(), 3);

        st.rename_criterion(1, "Hurtos");
        assert_eq!(names(&st)[1], "Hurtos");

        st.add_criterion();
        let rows = profile_rows(&st);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[5].name, "Criterio 6");
        assert_eq!(rows[5].cells.row_count(), 3);
        assert_eq!(rows[5].cells.row_data(0).unwrap().text, "0");
        // A new criterion has an automatic range (no values yet => 0).
        assert_eq!(range_cell(&st, 5).text, "0");
        assert!(!range_cell(&st, 5).manual);

        st.remove_criterion(0);
        assert_eq!(names(&st)[0], "Hurtos");
        assert_eq!(profile_rows(&st).len(), 5);
        // Ranges move with their criterion: "Hurtos" kept its manual maximum.
        assert_eq!(range_cell(&st, 0).text, "254572");
        assert!(range_cell(&st, 0).manual);
    }

    #[test]
    fn limiting_mode_has_a_column_without_profile_and_a_dash_in_the_classification() {
        let mut st = loaded("londres.ahp");
        st.set_profile_type(ProfileType::Limiting);
        let columns = profile_columns(&st);
        assert_eq!(columns.len(), 3);
        assert!(columns[0].has_profile && columns[1].has_profile && !columns[2].has_profile);
        assert_eq!(catch_all(&st), "Bajo");

        // Higher values preferred everywhere (left point = higher value wins): the
        // alternative with the highest values lands above every limit, in "Bajo".
        for values in &mut st.evaluations.per_criterion {
            values.iter_mut().for_each(|v| *v = 3.0);
        }
        st.jump_to_result().unwrap();
        assert_eq!(st.screen, Screen::Classification);
        let rows = classification_rows(&st);
        let westminster = rows.iter().find(|r| r.name == "Westminster").unwrap();
        assert_eq!(westminster.category, "Bajo");
        assert_eq!(westminster.profile_global, "—");
        assert_eq!(westminster.category_index, 2);
        // Rows of categories with a profile show that profile's global priority.
        let profiles = &st.sort_outcome().unwrap().profiles;
        for row in rows.iter().filter(|r| r.category != "Bajo") {
            let (_, _, g) = profiles.iter().find(|p| row.category == p.0).unwrap();
            assert_eq!(row.profile_global, fmt_weight(*g));
        }
    }

    #[test]
    fn local_tables_are_ascending_with_profile_labels_and_charts() {
        let mut st = loaded("londres.ahp");
        st.jump_to_result().unwrap();
        let tables = local_tables(&st);
        assert_eq!(tables.len(), 5);
        let first = &tables[0];
        assert_eq!(first.title, "Crímenes");
        assert_eq!(first.rows.row_count(), 9);
        let labels: Vec<String> = first.rows.iter().map(|r| r.label.to_string()).collect();
        assert_eq!(
            labels,
            [
                "RP1", "CP1", "RP2", "RP3", "CP2", "RP4", "CP3", "RP5", "RP6"
            ]
        );
        let xs: Vec<f64> = first.rows.iter().map(|r| r.x.parse().unwrap()).collect();
        assert!(xs.windows(2).all(|w| w[0] <= w[1]), "{xs:?}");
        assert_eq!(first.line.matches('L').count(), 8);
        assert_eq!(local_legend(&st), "CP1 = Alto · CP2 = Medio · CP3 = Bajo");
    }

    #[test]
    fn priorities_tables_have_one_column_per_criterion() {
        let mut st = loaded("londres.ahp");
        st.jump_to_result().unwrap();
        let o = st.sort_outcome().unwrap();
        let alternatives = result_rows(&o.alternatives);
        assert_eq!(alternatives.len(), 6);
        assert!(alternatives.iter().all(|r| r.cells.row_count() == 5));
        let profiles = result_rows(&o.profiles);
        let names: Vec<String> = profiles.iter().map(|r| r.name.to_string()).collect();
        assert_eq!(names, ["Alto", "Medio", "Bajo"]);
    }
}
