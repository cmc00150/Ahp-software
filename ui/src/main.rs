//! `ahp-ui`: desktop application for classic AHP and AHPSort II projects.
//!
//! Usage: `ahp-ui [file.ahp]`.
//!
//! Layout: `state.rs` holds the application state and logic, `convert.rs` pure conversions,
//! `chart.rs` the local-priority chart geometry, `models.rs` fills the Slint stores, and this
//! file wires the Slint callbacks to them.

mod chart;
mod convert;
mod models;
mod state;

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use ahp::{Method, ProfileType, WeightMethod};
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

/// Asks for a file name and saves. The dialog runs on its own thread (`AsyncFileDialog`) and is
/// awaited on the Slint event loop, so the window keeps answering while it is open; a blocking
/// dialog freezes the event loop and the desktop reports "La aplicación no responde".
fn save(ui: &AppWindow, state: &Shared) {
    let (dir, name) = {
        let st = state.borrow();
        // Same check as `save_to`, before asking for a file name.
        if let Some(blocker) = st.save_blocker() {
            models::show_message(ui, &blocker, true);
            return;
        }
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
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Guardar proyecto")
        .add_filter("Proyecto AHP", &["ahp"])
        .set_file_name(name);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    let weak = ui.as_weak();
    let state = state.clone();
    let spawned = slint::spawn_local(async move {
        let Some(handle) = dialog.save_file().await else {
            return;
        };
        let Some(ui) = weak.upgrade() else { return };
        let mut path = handle.path().to_path_buf();
        if path.extension().is_none_or(|e| e != "ahp") {
            path.set_extension("ahp");
        }
        let mut st = state.borrow_mut();
        match st.save_to(&path) {
            Ok(()) => models::show_message(&ui, &format!("Guardado en {}", path.display()), false),
            Err(e) => models::show_message(&ui, &e, true),
        }
        models::refresh_nav(&ui, &st);
    });
    if let Err(e) = spawned {
        models::show_message(ui, &format!("No se pudo abrir el diálogo: {e}"), true);
    }
}

fn wire_navigation(ui: &AppWindow, state: &Shared) {
    let nav = ui.global::<NavStore>();
    nav.on_go_to(handler!(ui, state, |ui, st, index: i32| {
        let result = match Screen::from_index(index, st.method) {
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
            Method::sortII
        });
        models::refresh(&ui, st);
    }));
    nav.on_set_weight_method(handler!(ui, state, |ui, st, index: i32| {
        let method = match index {
            1 => WeightMethod::GeometricMean,
            2 => WeightMethod::Approximated,
            _ => WeightMethod::EigenValues,
        };
        let result = st.set_weight_method(method);
        finish(&ui, st, result);
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
            // Same as `save`: an async dialog keeps the event loop (and the window) responsive.
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Cargar proyecto")
                .add_filter("Proyecto AHP", &["ahp"]);
            let task_weak = weak.clone();
            let state = state.clone();
            let spawned = slint::spawn_local(async move {
                let Some(handle) = dialog.pick_file().await else {
                    return;
                };
                if let Some(ui) = task_weak.upgrade() {
                    load(&ui, &mut state.borrow_mut(), handle.path().to_path_buf());
                }
            });
            if let (Err(e), Some(ui)) = (spawned, weak.upgrade()) {
                models::show_message(&ui, &format!("No se pudo abrir el diálogo: {e}"), true);
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
        |ui, st, c: i32, text: slint::SharedString| {
            st.rename_criterion(c as usize, &text);
            models::update_profile_row_name(&ui, st, c as usize);
        }
    ));
    data.on_set_unit(handler!(
        ui,
        state,
        |ui, st, c: i32, text: slint::SharedString| {
            st.set_unit(c as usize, &text);
            models::update_profile_row_name(&ui, st, c as usize);
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
            if st.method == Method::sortII {
                // An automatic range follows the maximum of the values.
                models::update_range(&ui, st, c as usize, None);
            }
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

fn wire_profiles(ui: &AppWindow, state: &Shared) {
    let profiles = ui.global::<ProfilesStore>();
    profiles.on_set_limiting(handler!(ui, state, |ui, st, limiting: bool| {
        st.set_profile_type(if limiting {
            ProfileType::Limiting
        } else {
            ProfileType::Central
        });
        models::refresh(&ui, st);
    }));
    profiles.on_rename_category(handler!(
        ui,
        state,
        |ui, st, k: i32, text: slint::SharedString| {
            st.rename_category(k as usize, &text);
            models::update_category_name(&ui, st);
        }
    ));
    profiles.on_add_category(handler!(ui, state, |ui, st| {
        st.add_category();
        models::refresh(&ui, st);
    }));
    profiles.on_remove_category(handler!(ui, state, |ui, st, k: i32| {
        st.remove_category(k as usize);
        models::refresh(&ui, st);
    }));
    profiles.on_set_range(handler!(
        ui,
        state,
        |ui, st, c: i32, text: slint::SharedString| {
            st.set_range(c as usize, &text);
            models::update_range(&ui, st, c as usize, Some(&text));
        }
    ));
    profiles.on_set_range_auto(handler!(ui, state, |ui, st, c: i32| {
        st.set_range_auto(c as usize);
        models::update_range(&ui, st, c as usize, None);
    }));
    profiles.on_set_profile(handler!(
        ui,
        state,
        |ui, st, c: i32, k: i32, text: slint::SharedString| {
            st.set_profile(c as usize, k as usize, &text);
            models::update_profile_cell(&ui, st, c as usize, k as usize);
        }
    ));
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
                    if st.method == Method::sortII {
                        models::refresh_local(&ui, st);
                    }
                }
                Err(e) => {
                    models::show_message(&ui, &e, true);
                    models::mark_matrix_cell_invalid(&ui, card, i, j, &text);
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
    wire_profiles(&ui, &state);
    wire_quiz(&ui, &state);
    wire_matrices(&ui, &state);

    match std::env::args_os().nth(1) {
        Some(path) => load(&ui, &mut state.borrow_mut(), PathBuf::from(path)),
        None => models::refresh(&ui, &state.borrow()),
    }
    ui.run()
}
