//! Application state: the project being edited, its evaluations and the navigation between
//! screens. Everything here is plain Rust (no Slint types) so it can be unit tested without a
//! window; `models.rs` turns it into Slint models.
//!
//! Both methods share the start, data, questionnaire and matrix screens; classic AHP ends with
//! a ranking and AHPSort II with the global priorities and the classification. AHPSort II also
//! edits the categories, their profiles and the range of each criterion on the data screen.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ahp::{
    Categories, ClassicOutcome, Criterion, Evaluations, Method, ProfileType, Project, ShortOutcome,
    solve_classic, solve_short,
};

use crate::convert::{pair_index, parse_number, parse_ratio, position_to_signed, set_pair};

/// Reference points RP1..RP6 of every AHPSort II range (as in the library).
pub const RANGE_POINTS: usize = 6;

/// Screens of the application. The declaration order is the navigation order inside each
/// method's flow (see [`Screen::flow`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Screen {
    Start,
    Data,
    Quiz,
    Matrices,
    /// Classic AHP: final ranking.
    Ranking,
    /// AHPSort II: local and global priorities of the alternatives and the profiles.
    Priorities,
    /// AHPSort II: category of each alternative.
    Classification,
}

impl Screen {
    const CLASSIC: [Screen; 5] = [
        Screen::Start,
        Screen::Data,
        Screen::Quiz,
        Screen::Matrices,
        Screen::Ranking,
    ];
    const SHORT: [Screen; 6] = [
        Screen::Start,
        Screen::Data,
        Screen::Quiz,
        Screen::Matrices,
        Screen::Priorities,
        Screen::Classification,
    ];

    /// Screens of `method`, in navigation order.
    pub fn flow(method: Method) -> &'static [Screen] {
        match method {
            Method::Classic => &Self::CLASSIC,
            Method::ShortII => &Self::SHORT,
        }
    }

    /// Position of the screen in its flow (what the Slint shell shows).
    pub fn index(self) -> i32 {
        match self {
            Screen::Start => 0,
            Screen::Data => 1,
            Screen::Quiz => 2,
            Screen::Matrices => 3,
            Screen::Ranking | Screen::Priorities => 4,
            Screen::Classification => 5,
        }
    }

    pub fn from_index(i: i32, method: Method) -> Option<Self> {
        usize::try_from(i)
            .ok()
            .and_then(|i| Self::flow(method).get(i).copied())
    }

    /// Final screen of `method` ("Ir al resultado").
    pub fn last(method: Method) -> Self {
        *Self::flow(method).last().expect("non-empty flow")
    }

    fn next(self, method: Method) -> Option<Self> {
        Self::from_index(self.index() + 1, method)
    }

    fn prev(self, method: Method) -> Option<Self> {
        Self::from_index(self.index() - 1, method)
    }
}

/// Result of the last solve.
#[derive(Debug, Clone)]
pub enum Outcome {
    Classic(ClassicOutcome),
    Short(ShortOutcome),
}

/// A change of the shape of the data table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureChange {
    AddCriterion,
    RemoveCriterion(usize),
    AddAlternative,
    RemoveAlternative(usize),
    /// Categories added or removed, or the profile type changed (AHPSort II): the range
    /// questionnaires whose number of points changed start again.
    Categories,
}

/// Identity of a point of an AHPSort II range questionnaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointKey {
    /// Reference point `RP{n + 1}`.
    Reference(usize),
    /// Profile of category `k`.
    Profile(usize),
}

/// Points of criterion `c` in questionnaire order, exactly as the library builds them:
/// RP1..RP6 (from 0 to the range maximum) followed by the profiles, sorted by value
/// descending with a stable sort.
pub fn range_point_order(project: &Project, c: usize) -> Vec<PointKey> {
    let Some(criterion) = project.criteria.get(c) else {
        return Vec::new();
    };
    let step = project.effective_range(c) / 5.0;
    let mut points = (0..RANGE_POINTS)
        .map(|n| (PointKey::Reference(n), n as f64 * step))
        .chain(
            criterion
                .profiles
                .iter()
                .enumerate()
                .map(|(k, &v)| (PointKey::Profile(k), v)),
        )
        .collect::<Vec<_>>();
    points.sort_by(|a, b| b.1.total_cmp(&a.1));
    points.into_iter().map(|(key, _)| key).collect()
}

/// Answers of a questionnaire whose points were reordered from `old` to `new`: every pair keeps
/// its answer, mirrored when its two points swapped sides. `None` when the points differ.
pub fn remap_pairs(values: &[f64], old: &[PointKey], new: &[PointKey]) -> Option<Vec<f64>> {
    let n = old.len();
    if new.len() != n || values.len() != n * n.saturating_sub(1) / 2 {
        return None;
    }
    let position = |key: &PointKey| old.iter().position(|k| k == key);
    let mut out = Vec::with_capacity(values.len());
    for i in 0..n {
        for j in i + 1..n {
            let (p, q) = (position(&new[i])?, position(&new[j])?);
            if p == q {
                return None;
            }
            let x = values[pair_index(n, p.min(q), p.max(q))];
            out.push(if p < q || x == 1.0 { x } else { -x });
        }
    }
    Some(out)
}

/// Categories of a new AHPSort II problem.
fn default_categories() -> Categories {
    Categories {
        profile_type: ProfileType::Central,
        names: (1..=3).map(|n| format!("Categoría {n}")).collect(),
    }
}

/// Gives an AHPSort II project categories (if it has none) and one profile per category with
/// a profile in every criterion. Returns whether some profile list had to be completed.
fn ensure_short_shape(project: &mut Project) -> bool {
    if project.categories.is_none() {
        project.categories = Some(default_categories());
    }
    let n = project.expected_profiles().unwrap_or(0);
    let mut fixed = false;
    for c in &mut project.criteria {
        if c.profiles.len() != n {
            c.profiles.resize(n, 0.0);
            fixed = true;
        }
    }
    fixed
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
        // The safety net below resets the questionnaires whose size changed.
        StructureChange::Categories => {}
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

/// Minimal problem: two criteria and two alternatives, all values 0 (plus three central
/// categories with their profiles at 0 for AHPSort II).
pub fn blank_project(method: Method) -> Project {
    let criterion = |n: usize| Criterion {
        name: format!("Criterio {n}"),
        values: vec![0.0; 2],
        ..Criterion::default()
    };
    let mut project = Project {
        method: Some(method),
        alternatives: vec!["Alternativa 1".into(), "Alternativa 2".into()],
        criteria: vec![criterion(1), criterion(2)],
        ..Project::default()
    };
    if method == Method::ShortII {
        ensure_short_shape(&mut project);
    }
    project
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
    /// AHPSort II range maxima (by criterion) whose text is not a number.
    pub invalid_ranges: BTreeMap<usize, String>,
    /// AHPSort II profile cells `(criterion, profile)` whose text is not a number.
    pub invalid_profiles: BTreeMap<(usize, usize), String>,
    /// Inline errors of the data screen (validation before continuing).
    pub data_errors: String,
    /// Result of the last solve; `None` when it must be recomputed.
    pub outcome: Option<Outcome>,
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
            invalid_ranges: BTreeMap::new(),
            invalid_profiles: BTreeMap::new(),
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
        // AHPSort II needs categories and one profile per category with a profile.
        if self.method == Method::ShortII && ensure_short_shape(&mut project) {
            warning.get_or_insert_with(|| {
                "Algunos criterios no tenían un perfil por categoría; se han completado con 0."
                    .into()
            });
        }
        self.replace_project(project, file, complete);
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
        self.invalid_ranges.clear();
        self.invalid_profiles.clear();
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
        if method == Method::ShortII {
            ensure_short_shape(&mut self.project);
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
        if !Screen::flow(method).contains(&self.screen) {
            self.screen = Screen::Data;
        }
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

    /// Why the project cannot be saved right now, if it cannot.
    pub fn save_blocker(&self) -> Option<String> {
        self.first_invalid_input()
            .map(|problem| format!("No se puede guardar: {problem} Corrígelo antes de guardar."))
    }

    /// Writes the project to `path`. Refused while some cell holds a text that is not a
    /// number: the file would silently keep the previous value of that cell.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        if let Some(blocker) = self.save_blocker() {
            return Err(blocker);
        }
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
        if self
            .project
            .criteria
            .get(c)
            .is_none_or(|crit| a >= crit.values.len())
        {
            return false;
        }
        match parse_number(text) {
            Ok(v) => {
                // An automatic range follows the values: its points may move.
                self.edit_points(|p| p.criteria[c].values[a] = v);
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
        self.invalid_profiles = shift_removed(&self.invalid_profiles, |cell| cell, c);
        self.invalid_ranges = self
            .invalid_ranges
            .iter()
            .filter(|&(&k, _)| k != c)
            .map(|(&k, text)| (if k > c { k - 1 } else { k }, text.clone()))
            .collect();
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

    // -----------------------------------------------------------------------------------------
    // AHPSort II: ranges, categories and profiles
    // -----------------------------------------------------------------------------------------

    /// Order of the range points of every criterion (questionnaire order).
    fn point_orders(&self) -> Vec<Vec<PointKey>> {
        (0..self.project.criteria.len())
            .map(|c| range_point_order(&self.project, c))
            .collect()
    }

    /// AHPSort II evaluations: the current ones or the ones kept aside while in classic mode.
    fn range_evaluations_mut(&mut self) -> Option<&mut Evaluations> {
        match (self.method, &mut self.stashed) {
            (Method::ShortII, _) => Some(&mut self.evaluations),
            (Method::Classic, Some((Method::ShortII, ev))) => Some(ev),
            _ => None,
        }
    }

    /// Applies an edit that may move range points (values of an automatic range, range
    /// maxima, profiles) and keeps every range answer attached to its pair of points.
    fn edit_points(&mut self, edit: impl FnOnce(&mut Project)) {
        let before = self.point_orders();
        edit(&mut self.project);
        let after = self.point_orders();
        let Some(ev) = self.range_evaluations_mut() else {
            return;
        };
        for ((old, new), values) in before.iter().zip(&after).zip(&mut ev.per_criterion) {
            if old != new
                && let Some(remapped) = remap_pairs(values, old, new)
            {
                *values = remapped;
            }
        }
    }

    /// Whether the range maximum of criterion `c` was set by hand (it no longer follows the
    /// values of the criterion).
    pub fn range_is_manual(&self, c: usize) -> bool {
        self.invalid_ranges.contains_key(&c)
            || self
                .project
                .criteria
                .get(c)
                .is_some_and(|c| c.range.is_some())
    }

    /// Sets the range maximum of criterion `c` by hand; returns whether `text` is valid.
    pub fn set_range(&mut self, c: usize, text: &str) -> bool {
        if c >= self.project.criteria.len() {
            return false;
        }
        match parse_number(text) {
            Ok(v) => {
                self.edit_points(|p| p.criteria[c].range = Some(v));
                self.invalid_ranges.remove(&c);
                self.outcome = None;
                true
            }
            Err(_) => {
                self.invalid_ranges.insert(c, text.to_string());
                false
            }
        }
    }

    /// The range maximum of criterion `c` follows the maximum of its values again.
    pub fn set_range_auto(&mut self, c: usize) {
        if c < self.project.criteria.len() {
            self.edit_points(|p| p.criteria[c].range = None);
            self.invalid_ranges.remove(&c);
            self.outcome = None;
        }
    }

    /// Sets profile `k` of criterion `c`; returns whether `text` is valid.
    pub fn set_profile(&mut self, c: usize, k: usize, text: &str) -> bool {
        if self
            .project
            .criteria
            .get(c)
            .is_none_or(|crit| k >= crit.profiles.len())
        {
            return false;
        }
        match parse_number(text) {
            Ok(v) => {
                self.edit_points(|p| p.criteria[c].profiles[k] = v);
                self.invalid_profiles.remove(&(c, k));
                self.outcome = None;
                true
            }
            Err(_) => {
                self.invalid_profiles.insert((c, k), text.to_string());
                false
            }
        }
    }

    pub fn categories(&self) -> Option<&Categories> {
        self.project.categories.as_ref()
    }

    pub fn rename_category(&mut self, k: usize, name: &str) {
        if let Some(slot) = self
            .project
            .categories
            .as_mut()
            .and_then(|c| c.names.get_mut(k))
        {
            *slot = name.to_string();
            self.outcome = None;
        }
    }

    /// Appends a category. Central: it gets a profile. Limiting: the new one becomes the
    /// category without profile and the previous last one gets a profile. Either way every
    /// criterion gets one more profile (at 0) at the end.
    pub fn add_category(&mut self) {
        let cats = self
            .project
            .categories
            .get_or_insert_with(default_categories);
        let name = next_default_name("Categoría", cats.names.iter());
        cats.names.push(name);
        for c in &mut self.project.criteria {
            c.profiles.push(0.0);
        }
        self.categories_changed();
    }

    /// Removes category `k` (at least two remain) and its profile. Removing the last category
    /// in limiting mode drops the profile of the one before, which becomes the last.
    pub fn remove_category(&mut self, k: usize) {
        let Some(cats) = self.project.categories.as_mut() else {
            return;
        };
        if k >= cats.names.len() || cats.names.len() <= 2 {
            return;
        }
        cats.names.remove(k);
        let profile = match cats.profile_type {
            ProfileType::Central => k,
            ProfileType::Limiting => k.min(cats.names.len() - 1),
        };
        for c in &mut self.project.criteria {
            if profile < c.profiles.len() {
                c.profiles.remove(profile);
            }
        }
        self.invalid_profiles = shift_removed(&self.invalid_profiles, |(c, p)| (p, c), profile);
        self.categories_changed();
    }

    /// Central: one profile per category. Limiting: the last category loses its profile
    /// (switching back gives it a profile at 0).
    pub fn set_profile_type(&mut self, profile_type: ProfileType) {
        let cats = self
            .project
            .categories
            .get_or_insert_with(default_categories);
        if cats.profile_type == profile_type {
            return;
        }
        cats.profile_type = profile_type;
        let n = self.project.expected_profiles().unwrap_or(0);
        for c in &mut self.project.criteria {
            c.profiles.resize(n, 0.0);
        }
        self.invalid_profiles.retain(|&(_, k), _| k < n);
        self.categories_changed();
    }

    fn categories_changed(&mut self) {
        self.structure_changed(StructureChange::Categories);
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
        let Some(target) = self.screen.next(self.method) else {
            return Ok(());
        };
        self.enter(target)?;
        self.max_reached = self.max_reached.max(target);
        Ok(())
    }

    /// "Atrás": the previous screen.
    pub fn back(&mut self) -> Result<(), String> {
        match self.screen.prev(self.method) {
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
        let last = Screen::last(self.method);
        self.enter(last)?;
        self.max_reached = last;
        Ok(())
    }

    /// Opens `target` after checking the data is ready for it (and solving when needed).
    fn enter(&mut self, target: Screen) -> Result<(), String> {
        if !Screen::flow(self.method).contains(&target) {
            return Err("Esa pantalla no existe para este método.".into());
        }
        if target >= Screen::Quiz {
            self.check_data()?;
            if target >= Screen::Matrices {
                self.solve()?;
            }
        }
        if target == Screen::Quiz {
            self.quiz_group = self.quiz_group.min(self.quiz_groups() - 1);
        }
        self.screen = target;
        Ok(())
    }

    /// Validates the data table; the errors are also kept for the data screen.
    /// First cell whose text is not a number (data table and, for AHPSort II, ranges and
    /// profiles), described for the user.
    pub fn first_invalid_input(&self) -> Option<String> {
        let criterion = |c: usize| {
            self.project
                .criteria
                .get(c)
                .map_or("?", |c| c.name.as_str())
        };
        if let Some(&(a, c)) = self.invalid_cells.keys().next() {
            return Some(format!(
                "El valor de \"{}\" en \"{}\" no es un número válido.",
                self.project.alternatives.get(a).map_or("?", String::as_str),
                criterion(c),
            ));
        }
        if self.method != Method::ShortII {
            return None;
        }
        if let Some(&c) = self.invalid_ranges.keys().next() {
            return Some(format!(
                "El máximo del rango de \"{}\" no es un número válido.",
                criterion(c)
            ));
        }
        self.invalid_profiles.keys().next().map(|&(c, k)| {
            let category = self
                .categories()
                .and_then(|cats| cats.names.get(k))
                .map_or("?", String::as_str);
            format!(
                "El perfil de \"{category}\" en \"{}\" no es un número válido.",
                criterion(c)
            )
        })
    }

    pub fn check_data(&mut self) -> Result<(), String> {
        let result = if let Some(problem) = self.first_invalid_input() {
            Err(problem)
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

    /// Solves the problem with the current method and evaluations.
    pub fn solve(&mut self) -> Result<(), String> {
        let outcome = match self.method {
            Method::Classic => {
                solve_classic(&self.project, &self.evaluations).map(Outcome::Classic)
            }
            Method::ShortII => solve_short(&self.project, &self.evaluations).map(Outcome::Short),
        };
        self.outcome = Some(outcome.map_err(|e| e.to_string())?);
        Ok(())
    }

    pub fn classic_outcome(&self) -> Option<&ClassicOutcome> {
        match &self.outcome {
            Some(Outcome::Classic(o)) => Some(o),
            _ => None,
        }
    }

    pub fn short_outcome(&self) -> Option<&ShortOutcome> {
        match &self.outcome {
            Some(Outcome::Short(o)) => Some(o),
            _ => None,
        }
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

    /// Edits cell `(i, j)` of matrix `card` (`0` = criteria, `k + 1` = alternatives (classic)
    /// or range points (AHPSort II) of criterion `k`): updates the reciprocal through the
    /// evaluations and solves again.
    pub fn edit_matrix(
        &mut self,
        card: usize,
        i: usize,
        j: usize,
        text: &str,
    ) -> Result<(), String> {
        let ratio = parse_ratio(text)?;
        let n = match (card, self.method) {
            (0, _) => self.project.criteria.len(),
            (_, Method::Classic) => self.project.alternatives.len(),
            (k, Method::ShortII) => self
                .project
                .criteria
                .get(k - 1)
                .map_or(0, |c| RANGE_POINTS + c.profiles.len()),
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
        let ranking = &st.classic_outcome().unwrap().ranking;
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
        let before = st
            .classic_outcome()
            .unwrap()
            .criteria
            .report
            .weights
            .clone();

        // a_10 (Consumo vs Precio) = 1/3 is the reciprocal of a_01 = 3: no change.
        st.edit_matrix(0, 1, 0, "1/3").unwrap();
        assert!((st.evaluations.criteria[0] - 3.0).abs() < 1e-12);

        // a_10 = 2 => Consumo dominates Precio: the pair (0,1) becomes -2.
        st.edit_matrix(0, 1, 0, "2").unwrap();
        assert_eq!(st.evaluations.criteria[0], -2.0);
        let outcome = st.classic_outcome().unwrap();
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
        // RP1..RP6 plus the three default profiles: 9 points, 36 pairs.
        let mut project = blank_project(Method::ShortII);
        let old = Evaluations {
            criteria: vec![3.0],
            per_criterion: vec![vec![2.0; 36], vec![-2.0; 36]],
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

    // ---------------------------------------------------------------------------------------
    // AHPSort II
    // ---------------------------------------------------------------------------------------

    fn short_blank() -> AppState {
        let mut st = AppState::new();
        st.set_method(Method::ShortII);
        st.new_problem();
        st
    }

    /// Answer of the pair "`a` vs `b`" in criterion `c` (positive: `a` preferred).
    fn answer(st: &AppState, c: usize, a: PointKey, b: PointKey) -> f64 {
        let order = range_point_order(&st.project, c);
        let (p, q) = (
            order.iter().position(|k| *k == a).unwrap(),
            order.iter().position(|k| *k == b).unwrap(),
        );
        let x = st.evaluations.per_criterion[c][pair_index(order.len(), p.min(q), p.max(q))];
        if p < q || x == 1.0 { x } else { -x }
    }

    fn all_keys(st: &AppState, c: usize) -> Vec<PointKey> {
        range_point_order(&st.project, c)
    }

    #[test]
    fn range_follows_the_values_until_it_is_edited_by_hand() {
        let mut st = short_blank();
        assert_eq!(st.project.effective_range(0), 0.0);
        assert!(!st.range_is_manual(0));

        // Automatic: the maximum of the values, updated on every edit.
        st.set_value(0, 0, "40");
        st.set_value(1, 0, "25");
        assert_eq!(st.project.effective_range(0), 40.0);
        st.set_value(0, 0, "10");
        assert_eq!(st.project.effective_range(0), 25.0);

        // Manual: sticks while the values change, and is saved in the file.
        assert!(st.set_range(0, "100"));
        assert!(st.range_is_manual(0));
        st.set_value(1, 0, "70");
        assert_eq!(st.project.effective_range(0), 100.0);
        assert_eq!(st.project_for_save().criteria[0].range, Some(100.0));

        // A rejected text keeps the previous maximum and blocks continuing.
        assert!(!st.set_range(0, "mucho"));
        assert!(st.range_is_manual(0));
        assert_eq!(st.project.effective_range(0), 100.0);
        assert!(st.advance().is_err());
        assert!(st.data_errors.contains("rango"), "{}", st.data_errors);

        // Back to automatic: follows the values again and is not written to the file.
        st.set_range_auto(0);
        assert!(!st.range_is_manual(0));
        assert_eq!(st.project.effective_range(0), 70.0);
        assert_eq!(st.project_for_save().criteria[0].range, None);
        assert!(st.invalid_ranges.is_empty());
    }

    #[test]
    fn profiles_follow_criteria_and_categories() {
        let mut st = short_blank();
        let shape = |st: &AppState| -> Vec<usize> {
            st.project
                .criteria
                .iter()
                .map(|c| c.profiles.len())
                .collect()
        };
        assert_eq!(st.categories().unwrap().names.len(), 3);
        assert_eq!(shape(&st), vec![3, 3]);

        st.add_criterion();
        assert_eq!(shape(&st), vec![3, 3, 3]);
        st.set_profile(2, 1, "7");
        st.rename_criterion(2, "Nuevo");
        assert_eq!(st.project.criteria[2].profiles, vec![0.0, 7.0, 0.0]);
        st.remove_criterion(0);
        assert_eq!(st.project.criteria[1].name, "Nuevo");
        assert_eq!(st.project.criteria[1].profiles, vec![0.0, 7.0, 0.0]);

        // Central: one profile per category.
        for (k, v) in ["1", "2", "3"].iter().enumerate() {
            assert!(st.set_profile(0, k, v));
        }
        st.add_category();
        assert_eq!(st.categories().unwrap().names[3], "Categoría 4");
        assert_eq!(st.project.criteria[0].profiles, vec![1.0, 2.0, 3.0, 0.0]);
        st.remove_category(1);
        assert_eq!(st.project.criteria[0].profiles, vec![1.0, 3.0, 0.0]);
        assert_eq!(
            st.categories().unwrap().names,
            ["Categoría 1", "Categoría 3", "Categoría 4"]
        );

        // Limiting: the last category has no profile.
        st.set_profile_type(ProfileType::Limiting);
        assert_eq!(st.project.criteria[0].profiles, vec![1.0, 3.0]);
        st.add_category();
        assert_eq!(st.project.criteria[0].profiles, vec![1.0, 3.0, 0.0]);
        // Removing the last one: the one before becomes the last and loses its profile.
        st.remove_category(3);
        assert_eq!(st.project.criteria[0].profiles, vec![1.0, 3.0]);
        st.remove_category(0);
        assert_eq!(st.project.criteria[0].profiles, vec![3.0]);
        // At least two categories remain.
        st.remove_category(0);
        assert_eq!(st.categories().unwrap().names.len(), 2);
        assert!(shape(&st).iter().all(|&n| n == 1));

        // Back to central: the last category gets a profile again.
        st.set_profile_type(ProfileType::Central);
        assert_eq!(st.project.criteria[0].profiles, vec![3.0, 0.0]);
        assert_eq!(st.project.expected_profiles(), Some(2));
        // The range questionnaires follow the number of points.
        assert!(st.evaluations.per_criterion.iter().all(|v| v.len() == 28));
    }

    #[test]
    fn invalid_profile_cells_move_with_their_category() {
        let mut st = short_blank();
        assert!(!st.set_profile(1, 2, "x"));
        assert!(st.advance().is_err());
        assert!(st.data_errors.contains("Categoría 3"), "{}", st.data_errors);
        st.remove_category(0);
        assert_eq!(
            st.invalid_profiles.keys().copied().collect::<Vec<_>>(),
            [(1, 1)]
        );
        st.set_profile_type(ProfileType::Limiting);
        assert!(st.invalid_profiles.is_empty());
    }

    #[test]
    fn point_order_matches_the_library_questionnaires() {
        let st = loaded("londres.ahp");
        let (_, per_criterion) = ahp::short_quizzes(&st.project).unwrap();
        let names = &st.categories().unwrap().names;
        for (c, (_, quiz)) in per_criterion.iter().enumerate() {
            let ours: Vec<String> = range_point_order(&st.project, c)
                .into_iter()
                .map(|k| match k {
                    PointKey::Reference(n) => format!("RP{}", n + 1),
                    PointKey::Profile(k) => names[k].clone(),
                })
                .collect();
            assert_eq!(ours, quiz.labels());
        }
    }

    #[test]
    fn moving_a_point_keeps_every_range_answer_on_its_pair() {
        let mut st = loaded("londres.ahp");
        let keys = all_keys(&st, 0);
        let pairs = |st: &AppState| -> Vec<f64> {
            keys.iter()
                .flat_map(|&a| keys.iter().map(move |&b| (a, b)))
                .filter(|(a, b)| a != b)
                .map(|(a, b)| answer(st, 0, a, b))
                .collect()
        };
        let before = pairs(&st);
        let order_before = all_keys(&st, 0);

        // "Alto" (20) moves above RP2 (41.2) and RP3 (82.4).
        assert!(st.set_profile(0, 0, "90"));
        assert_ne!(all_keys(&st, 0), order_before);
        assert_eq!(pairs(&st), before);

        // An automatic range moves every RP point with the values.
        st.set_range_auto(0);
        assert_eq!(pairs(&st), before);
        st.set_value(5, 0, "400");
        assert_eq!(pairs(&st), before);
        // The file still holds every answer (it stays "complete").
        assert!(st.can_jump_to_result);
        st.jump_to_result().unwrap();
    }

    #[test]
    fn londres_jumps_to_the_classification() {
        let mut st = loaded("londres.ahp");
        assert_eq!(st.method, Method::ShortII);
        assert!(st.can_jump_to_result);
        // A range present in the file means "set by hand".
        assert!((0..5).all(|c| st.range_is_manual(c)));

        st.jump_to_result().unwrap();
        assert_eq!(st.screen, Screen::Classification);
        assert_eq!(st.screen.index(), 5);
        assert_eq!(st.max_reached, Screen::Classification);
        let names = &st.categories().unwrap().names;
        let outcome = st.short_outcome().unwrap();
        assert_eq!(outcome.classification.len(), 6);
        assert!(
            outcome
                .classification
                .iter()
                .all(|(_, _, category)| names.contains(category)),
            "{:?}",
            outcome.classification
        );

        // Every step stays reachable, and "Atrás" walks the AHPSort II flow.
        st.navigate(Screen::Matrices).unwrap();
        st.advance().unwrap();
        assert_eq!(st.screen, Screen::Priorities);
        st.back().unwrap();
        assert_eq!(st.screen, Screen::Matrices);
        assert!(st.navigate(Screen::Ranking).is_err());
        assert_eq!(
            Screen::from_index(4, Method::ShortII),
            Some(Screen::Priorities)
        );
        assert_eq!(
            Screen::from_index(4, Method::Classic),
            Some(Screen::Ranking)
        );
        assert_eq!(Screen::from_index(5, Method::Classic), None);
    }

    #[test]
    fn partial_londres_walks_to_the_matrices_with_skip() {
        let mut st = loaded("londres_parcial.ahp");
        assert!(!st.can_jump_to_result);
        st.advance().unwrap();
        assert_eq!(st.screen, Screen::Quiz);
        assert_eq!(st.quiz_groups(), 6);
        st.quiz_next().unwrap();
        st.set_quiz_answer(1, 0, 4);
        assert_eq!(st.evaluations.per_criterion[0][0], 5.0);
        st.skip_quiz().unwrap();
        assert_eq!(st.screen, Screen::Matrices);
        let outcome = st.short_outcome().unwrap();
        assert_eq!(outcome.ranges.len(), 5);
        assert_eq!(outcome.ranges[0].1.labels.len(), 9);
        assert!(st.navigate(Screen::Priorities).is_err());
        st.advance().unwrap();
        st.advance().unwrap();
        assert_eq!(st.screen, Screen::Classification);
    }

    #[test]
    fn range_matrix_edit_changes_weights_and_classification_inputs() {
        let mut st = loaded("londres.ahp");
        st.jump_to_result().unwrap();
        let before = st.short_outcome().unwrap().clone();

        // Range matrix of "Crímenes" (card 1): RP6 (row 0) vs RP1 (column 8).
        st.edit_matrix(1, 0, 8, "5").unwrap();
        assert_eq!(st.evaluations.per_criterion[0][pair_index(9, 0, 8)], 5.0);
        let after = st.short_outcome().unwrap();
        assert_ne!(
            after.ranges[0].1.report.weights,
            before.ranges[0].1.report.weights
        );
        assert_ne!(after.local_priorities[0], before.local_priorities[0]);
        let globals =
            |o: &ShortOutcome| -> Vec<f64> { o.alternatives.iter().map(|a| a.2).collect() };
        assert_ne!(globals(after), globals(&before));
        let profiles = |o: &ShortOutcome| -> Vec<f64> { o.profiles.iter().map(|p| p.2).collect() };
        assert_ne!(profiles(after), profiles(&before));
        // Other criteria are untouched.
        assert_eq!(after.local_priorities[1], before.local_priorities[1]);

        // Reciprocal edit; the 9 x 9 size of a range matrix is honoured.
        st.edit_matrix(1, 8, 0, "1/7").unwrap();
        assert_eq!(st.evaluations.per_criterion[0][pair_index(9, 0, 8)], 7.0);
        assert!(st.edit_matrix(1, 0, 9, "3").is_err());
    }

    #[test]
    fn saving_is_refused_while_a_cell_is_invalid() {
        let dir = std::env::temp_dir().join(format!("ahp-ui-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("no.ahp");

        let mut st = loaded("coches.ahp");
        assert!(!st.set_value(0, 0, "doce"));
        let err = st.save_to(&path).unwrap_err();
        assert!(err.starts_with("No se puede guardar"), "{err}");
        assert!(err.contains("no es un número válido"), "{err}");
        assert!(!path.exists());
        assert_eq!(st.save_blocker(), Some(err));

        // AHPSort II: an invalid profile also blocks saving; fixing it allows it again.
        let mut st = loaded("londres.ahp");
        assert!(!st.set_profile(0, 0, "?"));
        assert!(st.save_to(&path).is_err());
        assert!(st.set_profile(0, 0, "25"));
        st.save_to(&path).unwrap();
        let mut again = AppState::new();
        again.load_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(again.project.criteria[0].profiles[0], 25.0);
        assert_eq!(again.evaluations, st.evaluations);
        assert!(again.can_jump_to_result);
    }

    #[test]
    fn switching_a_classic_problem_to_ahpsort_creates_categories() {
        let mut st = loaded("coches.ahp");
        st.set_method(Method::ShortII);
        let cats = st.categories().unwrap();
        assert_eq!(cats.profile_type, ProfileType::Central);
        assert_eq!(cats.names.len(), 3);
        assert!(st.project.criteria.iter().all(|c| c.profiles.len() == 3));
        assert!(st.evaluations.per_criterion.iter().all(|v| v.len() == 36));
        st.advance().unwrap();
        assert_eq!(st.screen, Screen::Quiz);
    }

    #[test]
    fn remap_pairs_mirrors_swapped_pairs() {
        use PointKey::*;
        let old = [Reference(1), Profile(0), Reference(0)];
        let new = [Profile(0), Reference(1), Reference(0)];
        // Pairs (0,1), (0,2), (1,2) in old order.
        let values = [3.0, 5.0, -2.0];
        assert_eq!(
            remap_pairs(&values, &old, &new),
            Some(vec![-3.0, -2.0, 5.0])
        );
        assert_eq!(
            remap_pairs(&[1.0, 1.0, 1.0], &old, &new),
            Some(vec![1.0; 3])
        );
        assert_eq!(remap_pairs(&values, &old, &new[..2]), None);
        assert_eq!(
            remap_pairs(&values, &old, &[Profile(1), Reference(1), Reference(0)]),
            None
        );
    }
}
