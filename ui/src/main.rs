//! `ahp-ui`: desktop application for classic AHP (and, later, AHPSort II) projects.
//!
//! Usage: `ahp-ui [file.ahp]`.
//!
//! Layout: `state.rs` holds the application state and logic, `convert.rs` pure conversions,
//! `models.rs` fills the Slint stores, and this file wires the Slint callbacks to them.

mod convert;
mod models;
mod state;

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use ahp::Method;
use slint::ComponentHandle;

use state::{AppState, Screen};

slint::include_modules!();

type Shared = Rc<RefCell<AppState>>;

/// Builds a callback closure that upgrades the window handle and borrows the state.
macro_rules! handler {
    ($ui:expr, $state:expr, |$u:ident, $s:ident $(, $arg:ident : $ty:ty)*| $body:block) => {{
        let weak = $ui.as_weak();
        let state = $state.clone();
        move |$($arg: $ty),*| {
            let Some($u) = weak.upgrade() else { return };
            let mut guard = state.borrow_mut();
            let $s: &mut AppState = &mut guard;
            $body
        }
    }};
}

/// Shows an error (if any) and refreshes the current screen.
fn finish(ui: &AppWindow, st: &AppState, result: Result<(), String>) {
    match result {
        Ok(()) => models::show_message(ui, "", false),
        Err(e) => models::show_message(ui, &e, true),
    }
    models::refresh(ui, st);
}

fn load(ui: &AppWindow, st: &mut AppState, path: PathBuf) {
    match st.load_file(&path) {
        Ok(None) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            models::show_message(ui, &format!("Se ha cargado «{name}»."), false);
        }
        Ok(Some(warning)) => models::show_message(ui, &warning, true),
        Err(e) => models::show_message(ui, &e, true),
    }
    models::refresh(ui, st);
}

fn save(ui: &AppWindow, state: &Shared) {
    let (dir, name) = {
        let st = state.borrow();
        let file = st.file.clone();
        let dir = file
            .as_ref()
            .and_then(|f| f.parent().map(|p| p.to_path_buf()));
        let name = file.as_ref().and_then(|f| f.file_name()).map_or_else(
            || "problema.ahp".into(),
            |n| n.to_string_lossy().into_owned(),
        );
        (dir, name)
    };
    let mut dialog = rfd::FileDialog::new()
        .set_title("Guardar proyecto")
        .add_filter("Proyecto AHP", &["ahp"])
        .set_file_name(name);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut path) = dialog.save_file() else {
        return;
    };
    if path.extension().is_none_or(|e| e != "ahp") {
        path.set_extension("ahp");
    }
    let mut st = state.borrow_mut();
    match st.save_to(&path) {
        Ok(()) => models::show_message(ui, &format!("Guardado en {}", path.display()), false),
        Err(e) => models::show_message(ui, &e, true),
    }
    models::refresh_nav(ui, &st);
}

fn wire_navigation(ui: &AppWindow, state: &Shared) {
    let nav = ui.global::<NavStore>();
    nav.on_go_to(handler!(ui, state, |ui, st, index: i32| {
        let result = match Screen::from_index(index) {
            Some(target) => st.navigate(target),
            None => Ok(()),
        };
        finish(&ui, st, result);
    }));
    nav.on_back(handler!(ui, state, |ui, st| {
        let result = st.back();
        finish(&ui, st, result);
    }));
    nav.on_next(handler!(ui, state, |ui, st| {
        let result = st.advance();
        finish(&ui, st, result);
    }));
    nav.on_new_problem(handler!(ui, state, |ui, st| {
        st.new_problem();
        finish(&ui, st, Ok(()));
    }));
    nav.on_set_classic(handler!(ui, state, |ui, st, classic: bool| {
        st.set_method(if classic {
            Method::Classic
        } else {
            Method::ShortII
        });
        models::refresh(&ui, st);
    }));
    nav.on_dismiss_message({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                models::show_message(&ui, "", false);
            }
        }
    });
    nav.on_load_file({
        let weak = ui.as_weak();
        let state = state.clone();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let picked = rfd::FileDialog::new()
                .set_title("Cargar proyecto")
                .add_filter("Proyecto AHP", &["ahp"])
                .pick_file();
            if let Some(path) = picked {
                load(&ui, &mut state.borrow_mut(), path);
            }
        }
    });
    nav.on_save({
        let weak = ui.as_weak();
        let state = state.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                save(&ui, &state);
            }
        }
    });
}

fn wire_data(ui: &AppWindow, state: &Shared) {
    let data = ui.global::<DataStore>();
    data.on_rename_criterion(handler!(
        ui,
        state,
        |_ui, st, c: i32, text: slint::SharedString| {
            st.rename_criterion(c as usize, &text);
        }
    ));
    data.on_set_unit(handler!(
        ui,
        state,
        |_ui, st, c: i32, text: slint::SharedString| {
            st.set_unit(c as usize, &text);
        }
    ));
    data.on_rename_alternative(handler!(
        ui,
        state,
        |_ui, st, a: i32, text: slint::SharedString| {
            st.rename_alternative(a as usize, &text);
        }
    ));
    data.on_set_value(handler!(
        ui,
        state,
        |ui, st, a: i32, c: i32, text: slint::SharedString| {
            st.set_value(a as usize, c as usize, &text);
            models::update_data_cell(&ui, st, a as usize, c as usize);
        }
    ));
    data.on_add_criterion(handler!(ui, state, |ui, st| {
        st.add_criterion();
        models::refresh(&ui, st);
    }));
    data.on_remove_criterion(handler!(ui, state, |ui, st, c: i32| {
        st.remove_criterion(c as usize);
        models::refresh(&ui, st);
    }));
    data.on_add_alternative(handler!(ui, state, |ui, st| {
        st.add_alternative();
        models::refresh(&ui, st);
    }));
    data.on_remove_alternative(handler!(ui, state, |ui, st, a: i32| {
        st.remove_alternative(a as usize);
        models::refresh(&ui, st);
    }));
    data.on_jump_to_result(handler!(ui, state, |ui, st| {
        let result = st.jump_to_result();
        finish(&ui, st, result);
    }));
}

fn wire_quiz(ui: &AppWindow, state: &Shared) {
    let quiz = ui.global::<QuizStore>();
    quiz.on_select(handler!(ui, state, |ui, st, item: i32, position: i32| {
        st.set_quiz_answer(st.quiz_group, item as usize, position);
        models::update_quiz_row(&ui, st, item as usize);
    }));
    quiz.on_go_to_group(handler!(ui, state, |ui, st, group: i32| {
        st.select_quiz_group(group as usize);
        models::refresh(&ui, st);
    }));
    quiz.on_previous_group(handler!(ui, state, |ui, st| {
        st.select_quiz_group(st.quiz_group.saturating_sub(1));
        models::refresh(&ui, st);
    }));
    quiz.on_next_group(handler!(ui, state, |ui, st| {
        let result = st.quiz_next();
        finish(&ui, st, result);
    }));
    quiz.on_skip(handler!(ui, state, |ui, st| {
        let result = st.skip_quiz();
        finish(&ui, st, result);
    }));
}

fn wire_matrices(ui: &AppWindow, state: &Shared) {
    let matrices = ui.global::<MatrixStore>();
    matrices.on_edit(handler!(
        ui,
        state,
        |ui, st, card: i32, i: i32, j: i32, text: slint::SharedString| {
            let (card, i, j) = (card as usize, i as usize, j as usize);
            match st.edit_matrix(card, i, j, &text) {
                Ok(()) => {
                    models::show_message(&ui, "", false);
                    models::update_matrix(&ui, st, card);
                }
                Err(e) => {
                    models::show_message(&ui, &e, true);
                    models::mark_matrix_cell_invalid(&ui, card, i, j);
                }
            }
        }
    ));
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let state: Shared = Rc::new(RefCell::new(AppState::new()));

    wire_navigation(&ui, &state);
    wire_data(&ui, &state);
    wire_quiz(&ui, &state);
    wire_matrices(&ui, &state);

    match std::env::args_os().nth(1) {
        Some(path) => load(&ui, &mut state.borrow_mut(), PathBuf::from(path)),
        None => models::refresh(&ui, &state.borrow()),
    }
    ui.run()
}
