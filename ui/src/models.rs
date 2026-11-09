//! Conversion of [`AppState`] into the Slint stores.
//!
//! `refresh` rebuilds the models of the current screen (used after navigation, loading and
//! structure changes). The `update_*` functions patch a single row/cell in place so inputs
//! keep their focus while the user edits.

use std::rc::Rc;

use ahp::{MatrixView, Method, QuizItem, Value, quizzes};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{
    AppWindow, DataCell, DataColumn, DataRow, DataStore, MatrixCardData, MatrixCell, MatrixStats,
    MatrixStore, NavStore, QuizRow, QuizStore, RankItem, RankingStore,
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
    // AHPSort II screens are placeholders for now (T4 adds their stores).
    if st.method != Method::Classic {
        return;
    }
    match st.screen {
        Screen::Start => {}
        Screen::Data => refresh_data(ui, st),
        Screen::Quiz => refresh_quiz(ui, st),
        Screen::Matrices => refresh_matrices(ui, st),
        Screen::Ranking => refresh_ranking(ui, st),
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
    let (criteria_quiz, per_criterion) = match quizzes(&st.project, Method::Classic) {
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
    } else {
        (
            format!("Criterio evaluado: {}", criterion_title(st, group - 1)),
            "Para cada par, indica qué alternativa es mejor según este criterio y cuánto."
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

/// Matrices in card order: criteria first, then one per criterion.
fn matrix_views(st: &AppState) -> Vec<(String, String, &MatrixView)> {
    let Some(outcome) = &st.outcome else {
        return Vec::new();
    };
    let mut cards = vec![(
        "Criterios".to_string(),
        "Importancia relativa de cada criterio".to_string(),
        &outcome.criteria,
    )];
    for (k, (_, view)) in outcome.alternatives.iter().enumerate() {
        let unit = st.project.criteria[k].unit.trim();
        let subtitle = if unit.is_empty() {
            String::new()
        } else {
            format!("Unidad: {unit}")
        };
        cards.push((
            format!("Alternativas según «{}»", st.project.criteria[k].name),
            subtitle,
            view,
        ));
    }
    cards
}

pub fn refresh_matrices(ui: &AppWindow, st: &AppState) {
    let store = ui.global::<MatrixStore>();
    let views = matrix_views(st);
    let cards = views
        .iter()
        .map(|(title, subtitle, view)| MatrixCardData {
            title: title.as_str().into(),
            subtitle: subtitle.as_str().into(),
            labels: strings(view.labels.iter().cloned()),
            rows: model(matrix_cells(view).into_iter().map(model).collect()),
        })
        .collect();
    let stats = views.iter().map(|(_, _, v)| matrix_stats(v)).collect();
    store.set_cards(model(cards));
    store.set_stats(model(stats));
}

/// Refreshes the cells and figures of one matrix in place (keeps the focused input).
pub fn update_matrix(ui: &AppWindow, st: &AppState, card: usize) {
    let store = ui.global::<MatrixStore>();
    let views = matrix_views(st);
    let (Some((_, _, view)), Some(data)) = (views.get(card), store.get_cards().row_data(card))
    else {
        return;
    };
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
    store.get_stats().set_row_data(card, matrix_stats(view));
}

/// Marks a matrix cell as invalid (the input keeps the text the user typed).
pub fn mark_matrix_cell_invalid(ui: &AppWindow, card: usize, i: usize, j: usize) {
    let cards = ui.global::<MatrixStore>().get_cards();
    if let Some(row) = cards.row_data(card).and_then(|d| d.rows.row_data(i))
        && let Some(mut cell) = row.row_data(j)
    {
        cell.valid = false;
        row.set_row_data(j, cell);
    }
}

// ---------------------------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------------------------

pub fn refresh_ranking(ui: &AppWindow, st: &AppState) {
    let ranking = st
        .outcome
        .as_ref()
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
