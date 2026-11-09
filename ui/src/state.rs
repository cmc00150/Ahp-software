//! Application state: the project being edited, its evaluations and the navigation between
//! screens. Everything here is plain Rust (no Slint types) so it can be unit tested without a
//! window; `models.rs` turns it into Slint models.
//!
//! Extension point for AHPSort II (T4): the `Method::ShortII` arms of [`AppState::enter`] and
//! the method-specific screens. Data editing, evaluation reshaping, loading and saving are
//! already method-aware.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ahp::{ClassicOutcome, Criterion, Evaluations, Method, Project, solve_classic};

use crate::convert::{parse_number, parse_ratio, position_to_signed, set_pair};

/// Screens of the application, in navigation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Screen {
    Start = 0,
    Data = 1,
    Quiz = 2,
    Matrices = 3,
    Ranking = 4,
}

impl Screen {
    const ALL: [Screen; 5] = [
        Screen::Start,
        Screen::Data,
        Screen::Quiz,
        Screen::Matrices,
        Screen::Ranking,
    ];

    pub fn index(self) -> i32 {
        self as i32
    }

    pub fn from_index(i: i32) -> Option<Self> {
        usize::try_from(i)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }

    fn next(self) -> Option<Self> {
        Self::from_index(self.index() + 1)
    }

    fn prev(self) -> Option<Self> {
        Self::from_index(self.index() - 1)
    }
}

/// A change of the shape of the data table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureChange {
    AddCriterion,
    RemoveCriterion(usize),
    AddAlternative,
    RemoveAlternative(usize),
}

/// Evaluations after a structure change, `project` being the already changed project.
///
/// A questionnaire whose shape did not change keeps its answers; one whose shape changed is
/// reset to "equal" (`1`). Renaming items or editing values never changes a shape.
/// - adding/removing a criterion resets the criteria questionnaire, keeps the questionnaires
///   of the remaining criteria and drops (or adds) the one of that criterion;
/// - adding/removing an alternative resets every alternatives questionnaire (classic); the
///   AHPSort II range questionnaires do not depend on the alternatives and are kept.
pub fn reshape_evaluations(
    old: &Evaluations,
    project: &Project,
    method: Method,
    change: StructureChange,
) -> Evaluations {
    let fresh = Evaluations::defaults(project, method);
    let mut criteria = old.criteria.clone();
    let mut per_criterion = old.per_criterion.clone();
    match change {
        StructureChange::AddCriterion => {
            criteria = fresh.criteria.clone();
            per_criterion.push(Vec::new());
        }
        StructureChange::RemoveCriterion(k) => {
            criteria = fresh.criteria.clone();
            if k < per_criterion.len() {
                per_criterion.remove(k);
            }
        }
        StructureChange::AddAlternative | StructureChange::RemoveAlternative(_) => {
            if method == Method::Classic {
                per_criterion = fresh.per_criterion.clone();
            }
        }
    }
    // Safety net: anything whose size no longer matches its questionnaire starts again.
    if criteria.len() != fresh.criteria.len() {
        criteria = fresh.criteria.clone();
    }
    per_criterion.resize(fresh.per_criterion.len(), Vec::new());
    for (slot, default) in per_criterion.iter_mut().zip(&fresh.per_criterion) {
        if slot.len() != default.len() {
            slot.clone_from(default);
        }
    }
    Evaluations {
        criteria,
        per_criterion,
    }
}

/// First `"{prefix} N"` (N = count + 1, then upwards) not already used.
fn next_default_name<'a>(
    prefix: &str,
    existing: impl Iterator<Item = &'a String> + Clone,
) -> String {
    let mut n = existing.clone().count() + 1;
    loop {
        let name = format!("{prefix} {n}");
        if !existing.clone().any(|e| *e == name) {
            return name;
        }
        n += 1;
    }
}

/// Minimal valid classic problem: two criteria and two alternatives, all values 0.
pub fn blank_project(method: Method) -> Project {
    let criterion = |n: usize| Criterion {
        name: format!("Criterio {n}"),
        values: vec![0.0; 2],
        ..Criterion::default()
    };
    Project {
        method: Some(method),
        alternatives: vec!["Alternativa 1".into(), "Alternativa 2".into()],
        criteria: vec![criterion(1), criterion(2)],
        ..Project::default()
    }
}

pub struct AppState {
    pub project: Project,
    pub method: Method,
    pub evaluations: Evaluations,
    /// File the project was loaded from or last saved to.
    pub file: Option<PathBuf>,
    /// `false` until the user creates or loads a problem.
    pub has_project: bool,
    pub screen: Screen,
    /// Furthest screen reached so far: the step header can jump forward up to it.
    pub max_reached: Screen,
    /// Current questionnaire: `0` = criteria, `k + 1` = alternatives under criterion `k`.
    pub quiz_group: usize,
    /// The loaded file had every evaluation: "Ir al resultado" is offered.
    pub can_jump_to_result: bool,
    /// Data cells `(alternative, criterion)` whose text is not a number, with that text.
    pub invalid_cells: BTreeMap<(usize, usize), String>,
    /// Inline errors of the data screen (validation before continuing).
    pub data_errors: String,
    /// Result of the last solve (classic); `None` when it must be recomputed.
    pub outcome: Option<ClassicOutcome>,
    /// Evaluations of the other method, kept while switching back and forth.
    stashed: Option<(Method, Evaluations)>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let project = blank_project(Method::Classic);
        let evaluations = Evaluations::defaults(&project, Method::Classic);
        Self {
            project,
            method: Method::Classic,
            evaluations,
            file: None,
            has_project: false,
            screen: Screen::Start,
            max_reached: Screen::Data,
            quiz_group: 0,
            can_jump_to_result: false,
            invalid_cells: BTreeMap::new(),
            data_errors: String::new(),
            outcome: None,
            stashed: None,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Problems and files
    // -----------------------------------------------------------------------------------------

    /// Starts a blank problem for the current method and opens the data screen.
    pub fn new_problem(&mut self) {
        let project = blank_project(self.method);
        self.replace_project(project, None, false);
    }

    /// Loads a `.ahp` file. `Ok(Some(warning))` means it was loaded but is not ready to solve.
    pub fn load_file(&mut self, path: &Path) -> Result<Option<String>, String> {
        let project = Project::load(path).map_err(|e| e.to_string())?;
        Ok(self.load_project(project, Some(path.to_path_buf())))
    }

    /// Takes a loaded project: the method comes from its hint (if any), evaluations from the
    /// file with missing parts set to "equal". Returns a warning when it does not validate.
    pub fn load_project(&mut self, mut project: Project, file: Option<PathBuf>) -> Option<String> {
        if let Some(method) = project.method {
            self.method = method;
        }
        let (evaluations, complete, mut warning) =
            match Evaluations::from_project(&project, self.method) {
                Ok((ev, missing)) => (ev, missing.is_complete(), None),
                Err(e) => (
                    Evaluations::defaults(&project, self.method),
                    false,
                    Some(format!(
                        "El fichero se ha cargado, pero no es válido para {}: {e}",
                        method_name(self.method)
                    )),
                ),
            };
        // The table needs one value per alternative in every criterion.
        let n = project.alternatives.len();
        if project.criteria.iter().any(|c| c.values.len() != n) {
            for c in &mut project.criteria {
                c.values.resize(n, 0.0);
            }
            warning.get_or_insert_with(|| {
                "Algunos criterios no tenían un valor por alternativa; se han completado con 0."
                    .into()
            });
        }
        self.replace_project(project, file, complete && self.method == Method::Classic);
        self.evaluations = evaluations;
        self.reshape_if_needed();
        warning
    }

    fn replace_project(&mut self, project: Project, file: Option<PathBuf>, complete: bool) {
        self.evaluations = Evaluations::defaults(&project, self.method);
        self.project = project;
        self.file = file;
        self.has_project = true;
        self.screen = Screen::Data;
        self.max_reached = Screen::Data;
        self.quiz_group = 0;
        self.can_jump_to_result = complete;
        self.invalid_cells.clear();
        self.data_errors.clear();
        self.outcome = None;
        self.stashed = None;
    }

    /// Defensive: makes every evaluation vector match its questionnaire size.
    fn reshape_if_needed(&mut self) {
        let fresh = Evaluations::defaults(&self.project, self.method);
        if self.evaluations.criteria.len() != fresh.criteria.len() {
            self.evaluations.criteria = fresh.criteria;
        }
        self.evaluations
            .per_criterion
            .resize(fresh.per_criterion.len(), Vec::new());
        for (slot, default) in self
            .evaluations
            .per_criterion
            .iter_mut()
            .zip(fresh.per_criterion)
        {
            if slot.len() != default.len() {
                *slot = default;
            }
        }
    }

    /// Switches method. The current evaluations are kept aside (switching back restores
    /// them); the ones of the new method are read from the project file data.
    pub fn set_method(&mut self, method: Method) {
        if method == self.method {
            return;
        }
        let incoming = match self.stashed.take() {
            Some((m, ev)) if m == method => ev,
            _ => Evaluations::from_project(&self.project, method)
                .map(|(ev, _)| ev)
                .unwrap_or_else(|_| Evaluations::defaults(&self.project, method)),
        };
        let previous = std::mem::replace(&mut self.evaluations, incoming);
        self.stashed = Some((self.method, previous));
        self.method = method;
        self.project.method = Some(method);
        self.reshape_if_needed();
        self.max_reached = self.max_reached.min(Screen::Data);
        self.can_jump_to_result = false;
        self.quiz_group = 0;
        self.outcome = None;
    }

    /// Project with the current evaluations, ready to be written to a file.
    pub fn project_for_save(&self) -> Project {
        let mut project = self.project.clone();
        sync_file_evaluations(&mut project, &self.evaluations, self.method);
        project
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        self.project_for_save()
            .save(path)
            .map_err(|e| e.to_string())?;
        self.file = Some(path.to_path_buf());
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Data table (rows = alternatives, columns = criteria)
    // -----------------------------------------------------------------------------------------

    pub fn rename_criterion(&mut self, c: usize, name: &str) {
        if let Some(criterion) = self.project.criteria.get_mut(c) {
            criterion.name = name.to_string();
            self.outcome = None;
        }
    }

    pub fn set_unit(&mut self, c: usize, unit: &str) {
        if let Some(criterion) = self.project.criteria.get_mut(c) {
            criterion.unit = unit.to_string();
            self.outcome = None;
        }
    }

    pub fn rename_alternative(&mut self, a: usize, name: &str) {
        if let Some(alternative) = self.project.alternatives.get_mut(a) {
            *alternative = name.to_string();
            self.outcome = None;
        }
    }

    /// Sets the value of alternative `a` under criterion `c`; returns whether `text` is valid
    /// (an invalid text keeps the previous value and marks the cell).
    pub fn set_value(&mut self, a: usize, c: usize, text: &str) -> bool {
        let Some(slot) = self
            .project
            .criteria
            .get_mut(c)
            .and_then(|crit| crit.values.get_mut(a))
        else {
            return false;
        };
        match parse_number(text) {
            Ok(v) => {
                *slot = v;
                self.invalid_cells.remove(&(a, c));
                self.outcome = None;
                true
            }
            Err(_) => {
                self.invalid_cells.insert((a, c), text.to_string());
                false
            }
        }
    }

    pub fn add_criterion(&mut self) {
        let name = next_default_name("Criterio", self.project.criteria.iter().map(|c| &c.name));
        let profiles = vec![0.0; self.project.expected_profiles().unwrap_or(0)];
        self.project.criteria.push(Criterion {
            name,
            values: vec![0.0; self.project.alternatives.len()],
            profiles,
            ..Criterion::default()
        });
        self.structure_changed(StructureChange::AddCriterion);
    }

    pub fn remove_criterion(&mut self, c: usize) {
        if c >= self.project.criteria.len() || self.project.criteria.len() <= 1 {
            return;
        }
        self.project.criteria.remove(c);
        self.invalid_cells = shift_removed(&self.invalid_cells, |(a, cc)| (cc, a), c);
        self.structure_changed(StructureChange::RemoveCriterion(c));
    }

    pub fn add_alternative(&mut self) {
        let name = next_default_name("Alternativa", self.project.alternatives.iter());
        self.project.alternatives.push(name);
        for c in &mut self.project.criteria {
            c.values.push(0.0);
        }
        self.structure_changed(StructureChange::AddAlternative);
    }

    pub fn remove_alternative(&mut self, a: usize) {
        if a >= self.project.alternatives.len() || self.project.alternatives.len() <= 1 {
            return;
        }
        self.project.alternatives.remove(a);
        for c in &mut self.project.criteria {
            if a < c.values.len() {
                c.values.remove(a);
            }
        }
        self.invalid_cells = shift_removed(&self.invalid_cells, |cell| cell, a);
        self.structure_changed(StructureChange::RemoveAlternative(a));
    }

    fn structure_changed(&mut self, change: StructureChange) {
        self.evaluations =
            reshape_evaluations(&self.evaluations, &self.project, self.method, change);
        // Some answers were reset: the file is no longer "complete".
        self.can_jump_to_result = false;
        self.stashed = None;
        self.quiz_group = 0;
        self.outcome = None;
    }

    // -----------------------------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------------------------

    /// Number of questionnaires: criteria plus one per criterion.
    pub fn quiz_groups(&self) -> usize {
        1 + self.project.criteria.len()
    }

    /// Jumps to a screen already reached (step header / start screen).
    pub fn navigate(&mut self, target: Screen) -> Result<(), String> {
        if target == self.screen {
            return Ok(());
        }
        if target > self.max_reached {
            return Err("Completa primero los pasos anteriores.".into());
        }
        if target != Screen::Start && !self.has_project {
            return Err("Crea un problema nuevo o carga un fichero primero.".into());
        }
        self.enter(target)
    }

    /// "Continuar": the next screen, which becomes reachable.
    pub fn advance(&mut self) -> Result<(), String> {
        let Some(target) = self.screen.next() else {
            return Ok(());
        };
        self.enter(target)?;
        self.max_reached = self.max_reached.max(target);
        Ok(())
    }

    /// "Atrás": the previous screen.
    pub fn back(&mut self) -> Result<(), String> {
        match self.screen.prev() {
            Some(target) => self.enter(target),
            None => Ok(()),
        }
    }

    /// "Saltar": from the questionnaires straight to the matrices (missing answers = equal).
    pub fn skip_quiz(&mut self) -> Result<(), String> {
        self.enter(Screen::Matrices)?;
        self.max_reached = self.max_reached.max(Screen::Matrices);
        Ok(())
    }

    /// "Ir al resultado": solves with the current evaluations and opens the last screen.
    pub fn jump_to_result(&mut self) -> Result<(), String> {
        self.enter(Screen::Ranking)?;
        self.max_reached = Screen::Ranking;
        Ok(())
    }

    /// Opens `target` after checking the data is ready for it (and solving when needed).
    fn enter(&mut self, target: Screen) -> Result<(), String> {
        if target >= Screen::Quiz {
            match self.method {
                Method::Classic => {
                    self.check_data()?;
                    if target >= Screen::Matrices {
                        self.solve()?;
                    }
                }
                // AHPSort II screens are not implemented yet (T4): the shell shows a
                // placeholder for every screen of that flow.
                Method::ShortII => {}
            }
        }
        if target == Screen::Quiz {
            self.quiz_group = self.quiz_group.min(self.quiz_groups() - 1);
        }
        self.screen = target;
        Ok(())
    }

    /// Validates the data table; the errors are also kept for the data screen.
    pub fn check_data(&mut self) -> Result<(), String> {
        let result = if let Some(&(a, c)) = self.invalid_cells.keys().next() {
            Err(format!(
                "El valor de \"{}\" en \"{}\" no es un número válido.",
                self.project.alternatives.get(a).map_or("?", String::as_str),
                self.project
                    .criteria
                    .get(c)
                    .map_or("?", |c| c.name.as_str()),
            ))
        } else {
            // The evaluations read from the file may refer to renamed or removed criteria:
            // validate the project with the current ones.
            sync_file_evaluations(&mut self.project, &self.evaluations, self.method);
            self.project
                .validate(self.method)
                .map_err(|e| e.to_string())
        };
        match &result {
            Ok(()) => self.data_errors.clear(),
            Err(e) => {
                self.data_errors = e.clone();
                self.screen = Screen::Data;
            }
        }
        result
    }

    /// Solves the classic problem with the current evaluations.
    pub fn solve(&mut self) -> Result<&ClassicOutcome, String> {
        let outcome = solve_classic(&self.project, &self.evaluations).map_err(|e| e.to_string())?;
        Ok(self.outcome.insert(outcome))
    }

    // -----------------------------------------------------------------------------------------
    // Questionnaires
    // -----------------------------------------------------------------------------------------

    pub fn select_quiz_group(&mut self, group: usize) {
        if group < self.quiz_groups() {
            self.quiz_group = group;
        }
    }

    /// "Siguiente" (next questionnaire) or, on the last one, "Continuar" (matrices).
    pub fn quiz_next(&mut self) -> Result<(), String> {
        if self.quiz_group + 1 < self.quiz_groups() {
            self.quiz_group += 1;
            Ok(())
        } else {
            self.advance()
        }
    }

    /// Evaluation vector of a questionnaire (`0` = criteria).
    pub fn group_values(&self, group: usize) -> Option<&[f64]> {
        match group {
            0 => Some(&self.evaluations.criteria),
            k => self.evaluations.per_criterion.get(k - 1).map(Vec::as_slice),
        }
    }

    fn group_values_mut(&mut self, group: usize) -> Option<&mut Vec<f64>> {
        match group {
            0 => Some(&mut self.evaluations.criteria),
            k => self.evaluations.per_criterion.get_mut(k - 1),
        }
    }

    /// Answer of a question: the dot `position` (0..=16) of the Saaty scale.
    pub fn set_quiz_answer(&mut self, group: usize, item: usize, position: i32) {
        if let Some(slot) = self.group_values_mut(group).and_then(|v| v.get_mut(item)) {
            *slot = position_to_signed(position);
            self.outcome = None;
        }
    }

    // -----------------------------------------------------------------------------------------
    // Matrices
    // -----------------------------------------------------------------------------------------

    /// Edits cell `(i, j)` of matrix `card` (`0` = criteria, `k + 1` = alternatives under
    /// criterion `k`): updates the reciprocal through the evaluations and solves again.
    pub fn edit_matrix(
        &mut self,
        card: usize,
        i: usize,
        j: usize,
        text: &str,
    ) -> Result<(), String> {
        let ratio = parse_ratio(text)?;
        let n = if card == 0 {
            self.project.criteria.len()
        } else {
            self.project.alternatives.len()
        };
        let previous = self.evaluations.clone();
        let values = self.group_values_mut(card).ok_or("esa matriz no existe")?;
        set_pair(values, n, i, j, ratio)?;
        if let Err(e) = self.solve() {
            self.evaluations = previous;
            return Err(e);
        }
        Ok(())
    }
}

/// Writes `ev` into the evaluations section of `project` (keyed by the current criterion
/// names) and drops the other method's evaluations of criteria that no longer exist.
fn sync_file_evaluations(project: &mut Project, ev: &Evaluations, method: Method) {
    project.method = Some(method);
    ev.store_into(project, method);
    let names = project.criteria_names();
    let other = match method {
        Method::Classic => &mut project.evaluations.ranges,
        Method::ShortII => &mut project.evaluations.alternatives,
    };
    other.0.retain(|(name, _)| names.contains(name));
}

/// Drops the cells whose row (or column) is `removed` and shifts the following ones.
/// `swap` puts the index being removed first and is its own inverse.
fn shift_removed(
    cells: &BTreeMap<(usize, usize), String>,
    swap: impl Fn((usize, usize)) -> (usize, usize),
    removed: usize,
) -> BTreeMap<(usize, usize), String> {
    cells
        .iter()
        .map(|(&cell, text)| (swap(cell), text))
        .filter(|&((k, _), _)| k != removed)
        .map(|((k, other), text)| {
            let k = if k > removed { k - 1 } else { k };
            (swap((k, other)), text.clone())
        })
        .collect()
}

pub fn method_name(method: Method) -> &'static str {
    match method {
        Method::Classic => "AHP clásico",
        Method::ShortII => "AHPSort II",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(name: &str) -> PathBuf {
        PathBuf::from(format!("{}/../examples/{name}", env!("CARGO_MANIFEST_DIR")))
    }

    fn loaded(name: &str) -> AppState {
        let mut st = AppState::new();
        let warning = st.load_file(&example(name)).unwrap();
        assert_eq!(warning, None);
        st
    }

    #[test]
    fn loads_the_complete_example_and_jumps_to_a_descending_ranking() {
        let mut st = loaded("coches.ahp");
        assert_eq!(st.method, Method::Classic);
        assert_eq!(st.screen, Screen::Data);
        assert!(st.can_jump_to_result);
        assert_eq!(st.evaluations.criteria, vec![3.0, 5.0, 7.0, 2.0, 4.0, 2.0]);

        st.jump_to_result().unwrap();
        assert_eq!(st.screen, Screen::Ranking);
        assert_eq!(st.max_reached, Screen::Ranking);
        let ranking = &st.outcome.as_ref().unwrap().ranking;
        assert_eq!(ranking.len(), 4);
        assert!(ranking.windows(2).all(|w| w[0].1 >= w[1].1), "{ranking:?}");
        let total: f64 = ranking.iter().map(|r| r.1).sum();
        assert!((total - 1.0).abs() < 1e-6);

        // Every step stays reachable afterwards.
        st.navigate(Screen::Quiz).unwrap();
        st.navigate(Screen::Matrices).unwrap();
        st.navigate(Screen::Ranking).unwrap();
    }

    #[test]
    fn unevaluated_file_does_not_offer_the_result() {
        let st = loaded("coches_sin_evaluar.ahp");
        assert!(!st.can_jump_to_result);
        assert!(st.evaluations.criteria.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn navigation_only_moves_forward_to_reached_screens() {
        let mut st = AppState::new();
        st.new_problem();
        assert!(st.navigate(Screen::Matrices).is_err());
        st.advance().unwrap();
        assert_eq!(st.screen, Screen::Quiz);
        st.skip_quiz().unwrap();
        assert_eq!(st.screen, Screen::Matrices);
        st.back().unwrap();
        st.back().unwrap();
        st.back().unwrap();
        assert_eq!(st.screen, Screen::Start);
        st.navigate(Screen::Matrices).unwrap();
        assert!(st.navigate(Screen::Ranking).is_err());
        st.advance().unwrap();
        assert_eq!(st.screen, Screen::Ranking);
    }

    #[test]
    fn continuing_with_invalid_data_stays_on_the_data_screen() {
        let mut st = AppState::new();
        st.new_problem();
        st.rename_criterion(1, "Criterio 1");
        assert!(st.advance().is_err());
        assert_eq!(st.screen, Screen::Data);
        assert!(st.data_errors.contains("repetid"), "{}", st.data_errors);

        st.rename_criterion(1, "Otro");
        assert!(!st.set_value(0, 0, "abc"));
        assert!(st.advance().is_err());
        assert!(st.data_errors.contains("Alternativa 1"));
        assert!(st.set_value(0, 0, "4,5"));
        assert_eq!(st.project.criteria[0].values[0], 4.5);
        st.advance().unwrap();
        assert!(st.data_errors.is_empty());
    }

    #[test]
    fn quiz_answers_are_stored_and_the_last_group_continues() {
        let mut st = loaded("coches_sin_evaluar.ahp");
        st.advance().unwrap();
        st.set_quiz_answer(0, 2, 0);
        st.set_quiz_answer(1, 0, 16);
        assert_eq!(st.evaluations.criteria[2], 9.0);
        assert_eq!(st.evaluations.per_criterion[0][0], -9.0);
        for _ in 0..st.quiz_groups() - 1 {
            st.quiz_next().unwrap();
            assert_eq!(st.screen, Screen::Quiz);
        }
        st.quiz_next().unwrap();
        assert_eq!(st.screen, Screen::Matrices);
    }

    #[test]
    fn matrix_edit_updates_the_reciprocal_and_the_weights() {
        let mut st = loaded("coches.ahp");
        st.jump_to_result().unwrap();
        let before = st.outcome.as_ref().unwrap().criteria.report.weights.clone();

        // a_10 (Consumo vs Precio) = 1/3 is the reciprocal of a_01 = 3: no change.
        st.edit_matrix(0, 1, 0, "1/3").unwrap();
        assert!((st.evaluations.criteria[0] - 3.0).abs() < 1e-12);

        // a_10 = 2 => Consumo dominates Precio: the pair (0,1) becomes -2.
        st.edit_matrix(0, 1, 0, "2").unwrap();
        assert_eq!(st.evaluations.criteria[0], -2.0);
        let outcome = st.outcome.as_ref().unwrap();
        assert!((outcome.criteria.cells[0][1] - 0.5).abs() < 1e-12);
        assert!((outcome.criteria.cells[1][0] - 2.0).abs() < 1e-12);
        assert_ne!(outcome.criteria.report.weights, before);

        // Decimal on an alternatives matrix.
        st.edit_matrix(2, 0, 3, "0.25").unwrap();
        assert_eq!(st.evaluations.per_criterion[1][2], -4.0);

        let snapshot = st.evaluations.clone();
        assert!(st.edit_matrix(0, 0, 1, "0").is_err());
        assert!(st.edit_matrix(0, 0, 1, "x").is_err());
        assert!(st.edit_matrix(0, 2, 2, "3").is_err());
        assert_eq!(st.evaluations, snapshot);
    }

    #[test]
    fn structure_changes_keep_the_answers_that_still_fit() {
        let mut st = loaded("coches.ahp");
        let original = st.evaluations.clone();

        // Renaming and editing values keeps everything.
        st.rename_criterion(0, "Coste");
        st.rename_alternative(1, "Golf GTI");
        st.set_value(0, 0, "19000");
        assert_eq!(st.evaluations, original);
        assert!(st.can_jump_to_result);

        // Removing criterion 1: criteria quiz reset, the others kept.
        st.remove_criterion(1);
        assert_eq!(st.evaluations.criteria, vec![1.0; 3]);
        assert_eq!(st.evaluations.per_criterion.len(), 3);
        assert_eq!(st.evaluations.per_criterion[0], original.per_criterion[0]);
        assert_eq!(st.evaluations.per_criterion[1], original.per_criterion[2]);
        assert_eq!(st.evaluations.per_criterion[2], original.per_criterion[3]);
        assert!(!st.can_jump_to_result);

        // Adding a criterion: criteria quiz reset, new questionnaire at "equal".
        st.set_quiz_answer(0, 0, 3);
        st.add_criterion();
        assert_eq!(st.project.criteria[3].name, "Criterio 4");
        assert_eq!(st.project.criteria[3].values, vec![0.0; 4]);
        assert_eq!(st.evaluations.criteria, vec![1.0; 6]);
        assert_eq!(st.evaluations.per_criterion[3], vec![1.0; 6]);
        assert_eq!(st.evaluations.per_criterion[0], original.per_criterion[0]);

        // Adding an alternative resets the alternatives questionnaires only.
        st.set_quiz_answer(0, 0, 3);
        st.add_alternative();
        assert_eq!(st.project.alternatives[4], "Alternativa 5");
        assert_eq!(st.evaluations.criteria[0], 6.0);
        assert!(
            st.evaluations
                .per_criterion
                .iter()
                .all(|v| v == &vec![1.0; 10])
        );

        st.remove_alternative(0);
        assert_eq!(st.project.alternatives.len(), 4);
        assert!(st.project.criteria.iter().all(|c| c.values.len() == 4));
        assert_eq!(st.evaluations.criteria[0], 6.0);
        assert!(st.evaluations.per_criterion.iter().all(|v| v.len() == 6));
        st.advance().unwrap();
        st.skip_quiz().unwrap();
    }

    #[test]
    fn reshape_keeps_range_questionnaires_for_ahpsort() {
        let mut project = blank_project(Method::ShortII);
        let old = Evaluations {
            criteria: vec![3.0],
            per_criterion: vec![vec![2.0; 15], vec![-2.0; 15]],
        };
        project.alternatives.push("Alternativa 3".into());
        let ev = reshape_evaluations(
            &old,
            &project,
            Method::ShortII,
            StructureChange::AddAlternative,
        );
        assert_eq!(ev, old);
    }

    #[test]
    fn invalid_cells_follow_removed_rows_and_columns() {
        let mut st = AppState::new();
        st.new_problem();
        st.add_alternative();
        st.add_criterion();
        st.set_value(2, 2, "?");
        st.set_value(0, 1, "?");
        st.remove_alternative(1);
        let keys = |st: &AppState| st.invalid_cells.keys().copied().collect::<Vec<_>>();
        assert_eq!(keys(&st), vec![(0, 1), (1, 2)]);
        assert_eq!(st.invalid_cells[&(1, 2)], "?");
        st.remove_criterion(1);
        assert_eq!(keys(&st), vec![(1, 1)]);
        // The last row/column cannot be removed.
        st.remove_criterion(0);
        st.remove_criterion(0);
        assert_eq!(st.project.criteria.len(), 1);
    }

    #[test]
    fn saving_round_trips_the_current_evaluations() {
        let mut st = loaded("coches_sin_evaluar.ahp");
        st.advance().unwrap();
        st.set_quiz_answer(0, 0, 2);
        let dir = std::env::temp_dir().join(format!("ahp-ui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("guardado.ahp");
        st.save_to(&path).unwrap();

        let mut again = AppState::new();
        again.load_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(again.evaluations, st.evaluations);
        assert!(again.can_jump_to_result);
        assert_eq!(again.method, Method::Classic);
    }

    #[test]
    fn switching_method_preserves_classic_answers() {
        let mut st = loaded("coches.ahp");
        let classic = st.evaluations.clone();
        st.set_method(Method::ShortII);
        assert_eq!(st.method, Method::ShortII);
        st.set_method(Method::Classic);
        assert_eq!(st.evaluations, classic);
    }
}
