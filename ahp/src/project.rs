//! `.ahp` project files (TOML) and a high-level solve API shared by every interface.
//!
//! A project holds the problem data (alternatives, criteria with their values, AHPSort II
//! ranges/profiles/categories) and, optionally, the pairwise evaluations. Evaluations are
//! signed Saaty values in quiz order (`(0,1), (0,2), ..., (1,2), ...`): `x >= 1` means the
//! left item dominates (`a_ij = x`) and `x <= -1` that the right one does (`a_ij = 1/|x|`).
//!
//! Minimum sizes (a pairwise comparison needs at least one pair at each level):
//! - classic: at least 2 criteria and 2 alternatives;
//! - AHPSort II: at least 2 criteria, 1 alternative and 2 categories.

use std::{collections::HashSet, fmt, path::Path};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::SerializeMap,
};

use crate::{
    ahp_classic::ahp::Ahp,
    ahp_sortII::{ahp_sortII::Ahpsort, profile::Profile},
    share::{
        pesos::{WeightMethod, WeightsReport},
        quiz::{QuizPairMatrix, signed_to_ratio},
        share::{Table, Value},
    },
};

/// Number of equidistant reference points (RP1..RP6) per AHPSort II criterion.
const RANGE_POINTS: usize = 6;

/// Criteria quiz plus one `(criterion, quiz)` per criterion.
pub type QuizSet = (QuizPairMatrix, Vec<(String, QuizPairMatrix)>);
/// Upper-triangle cells `(row, col, ratio)` of a pairwise matrix.
type UpperCells = Vec<(usize, usize, f64)>;
/// Rows returned by the library's global-priorities functions.
type PriorityRows<'a> = Vec<(&'a str, Vec<(&'a str, f64)>, f64)>;

// ---------------------------------------------------------------------------------------------
// File format
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    #[serde(rename = "classic")]
    Classic,
    #[serde(rename = "sortII")]
    sortII,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Method::Classic => "classic",
            Method::sortII => "sortII",
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Optional hint of the method the file was written for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<Method>,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub criteria: Vec<Criterion>,
    /// AHPSort II only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Categories>,
    #[serde(default, skip_serializing_if = "FileEvaluations::is_empty")]
    pub evaluations: FileEvaluations,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    /// One value per alternative.
    #[serde(default)]
    pub values: Vec<f64>,
    /// AHPSort II: maximum of the range; `None` => automatic (max of `values`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<f64>,
    /// AHPSort II: one value per category that has a profile, in category order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileType {
    /// One profile per category.
    Central,
    /// One profile per category except the last one, which collects everything above.
    Limiting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Categories {
    pub profile_type: ProfileType,
    pub names: Vec<String>,
}

/// Evaluations stored in the file; every part is optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEvaluations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<Vec<f64>>,
    /// Classic: alternatives quiz of each criterion, keyed by criterion name.
    #[serde(default, skip_serializing_if = "NamedVectors::is_empty")]
    pub alternatives: NamedVectors,
    /// AHPSort II: range-points quiz of each criterion, keyed by criterion name.
    #[serde(default, skip_serializing_if = "NamedVectors::is_empty")]
    pub ranges: NamedVectors,
}

impl FileEvaluations {
    pub fn is_empty(&self) -> bool {
        self.criteria.is_none() && self.alternatives.is_empty() && self.ranges.is_empty()
    }
}

/// A TOML table `name = [values]` that keeps the order of its entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NamedVectors(pub Vec<(String, Vec<f64>)>);

impl NamedVectors {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&[f64]> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }
}

impl Serialize for NamedVectors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, values) in &self.0 {
            map.serialize_entry(name, values)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for NamedVectors {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NamedVisitor;

        impl<'de> Visitor<'de> for NamedVisitor {
            type Value = NamedVectors;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a table of criterion name = [numbers]")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut entries: Vec<(String, Vec<f64>)> = Vec::new();
                while let Some((name, values)) = access.next_entry::<String, Vec<f64>>()? {
                    if entries.iter().any(|(n, _)| *n == name) {
                        return Err(serde::de::Error::custom(format!(
                            "criterio repetido: {name}"
                        )));
                    }
                    entries.push((name, values));
                }
                Ok(NamedVectors(entries))
            }
        }

        deserializer.deserialize_map(NamedVisitor)
    }
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectError {
    /// The file could not be read or written.
    Io(String),
    /// The text is not a valid `.ahp` TOML document.
    Parse(String),
    /// The project could not be serialized.
    Serialize(String),
    /// The data is inconsistent for the requested method.
    Invalid(String),
    /// The AHP library rejected the data.
    Library(&'static str),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io(m) => write!(f, "Error de lectura/escritura: {m}"),
            ProjectError::Parse(m) => write!(f, "Fichero .ahp inválido: {m}"),
            ProjectError::Serialize(m) => write!(f, "No se pudo generar el fichero: {m}"),
            ProjectError::Invalid(m) => write!(f, "Proyecto inválido: {m}"),
            ProjectError::Library(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<&'static str> for ProjectError {
    fn from(m: &'static str) -> Self {
        ProjectError::Library(m)
    }
}

fn invalid(msg: impl Into<String>) -> ProjectError {
    ProjectError::Invalid(msg.into())
}

// ---------------------------------------------------------------------------------------------
// Project: I/O and validation
// ---------------------------------------------------------------------------------------------

impl Project {
    pub fn from_toml_str(text: &str) -> Result<Self, ProjectError> {
        toml::from_str(text).map_err(|e| ProjectError::Parse(e.to_string()))
    }

    pub fn to_toml_string(&self) -> Result<String, ProjectError> {
        let text = toml::to_string(self).map_err(|e| ProjectError::Serialize(e.to_string()))?;
        // Blank line before each table header for readability (skipped if the text has
        // multi-line strings, where a line starting with `[` could be string content).
        if text.contains("\"\"\"") || text.contains("'''") {
            return Ok(text);
        }
        let mut out = String::with_capacity(text.len() + 64);
        for line in text.lines() {
            if line.starts_with('[') && !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
        }
        Ok(out)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))?;
        Self::from_toml_str(&text)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        let path = path.as_ref();
        std::fs::write(path, self.to_toml_string()?)
            .map_err(|e| ProjectError::Io(format!("{}: {e}", path.display())))
    }

    pub fn criteria_names(&self) -> Vec<String> {
        self.criteria.iter().map(|c| c.name.clone()).collect()
    }

    /// Maximum of the range of criterion `i`: the manual `range` if set, otherwise the maximum
    /// of its values (`0.0` when it has no values or does not exist).
    pub fn effective_range(&self, i: usize) -> f64 {
        let Some(c) = self.criteria.get(i) else {
            return 0.0;
        };
        c.range
            .unwrap_or_else(|| c.values.iter().copied().reduce(f64::max).unwrap_or(0.0))
    }

    /// Number of category profiles each AHPSort II criterion must have.
    pub fn expected_profiles(&self) -> Option<usize> {
        self.categories.as_ref().map(|c| match c.profile_type {
            ProfileType::Central => c.names.len(),
            ProfileType::Limiting => c.names.len().saturating_sub(1),
        })
    }

    pub fn validate(&self, method: Method) -> Result<(), ProjectError> {
        match method {
            Method::Classic => self.validate_classic(),
            Method::sortII => self.validate_sort(),
        }
    }

    /// Checks the project can be solved with classic AHP.
    pub fn validate_classic(&self) -> Result<(), ProjectError> {
        self.validate_common(2)?;
        self.validate_evaluations(Method::Classic)
    }

    /// Checks the project can be solved with AHPSort II.
    pub fn validate_sort(&self) -> Result<(), ProjectError> {
        self.validate_common(1)?;
        let cats = self
            .categories
            .as_ref()
            .ok_or_else(|| invalid("AHPSort II necesita una sección [categories]"))?;
        if cats.names.len() < 2 {
            return Err(invalid("se necesitan al menos 2 categorías"));
        }
        unique_names(&cats.names, "categoría")?;
        let n_profiles = self.expected_profiles().unwrap_or(0);
        for (i, c) in self.criteria.iter().enumerate() {
            if c.profiles.len() != n_profiles {
                return Err(invalid(format!(
                    "el criterio \"{}\" tiene {} perfiles y con categorías de tipo {:?} debe tener {n_profiles}",
                    c.name,
                    c.profiles.len(),
                    cats.profile_type,
                )));
            }
            if c.profiles.iter().any(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "el criterio \"{}\" tiene perfiles no numéricos",
                    c.name
                )));
            }
            let range = self.effective_range(i);
            if !(range.is_finite() && range > 0.0) {
                return Err(invalid(format!(
                    "el rango del criterio \"{}\" debe ser mayor que 0",
                    c.name
                )));
            }
        }
        self.validate_evaluations(Method::sortII)
    }

    fn validate_common(&self, min_alternatives: usize) -> Result<(), ProjectError> {
        if self.criteria.len() < 2 {
            return Err(invalid("se necesitan al menos 2 criterios"));
        }
        if self.alternatives.len() < min_alternatives {
            return Err(invalid(format!(
                "se necesitan al menos {min_alternatives} alternativas"
            )));
        }
        unique_names(&self.alternatives, "alternativa")?;
        unique_names(&self.criteria_names(), "criterio")?;
        for c in &self.criteria {
            if c.values.len() != self.alternatives.len() {
                return Err(invalid(format!(
                    "el criterio \"{}\" tiene {} valores y hay {} alternativas",
                    c.name,
                    c.values.len(),
                    self.alternatives.len()
                )));
            }
            if c.values.iter().any(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "el criterio \"{}\" tiene valores no numéricos",
                    c.name
                )));
            }
        }
        Ok(())
    }

    fn validate_evaluations(&self, method: Method) -> Result<(), ProjectError> {
        let (criteria_len, per_criterion_len) = self.quiz_sizes(method);
        let ev = &self.evaluations;
        check_signed("evaluations.criteria", ev.criteria.as_deref(), criteria_len)?;

        let names = self.criteria_names();
        for (section, map) in [("alternatives", &ev.alternatives), ("ranges", &ev.ranges)] {
            if let Some((key, _)) = map.0.iter().find(|(k, _)| !names.contains(k)) {
                return Err(invalid(format!(
                    "evaluations.{section} contiene \"{key}\", que no es un criterio"
                )));
            }
        }
        let map = match method {
            Method::Classic => &ev.alternatives,
            Method::sortII => &ev.ranges,
        };
        for (name, len) in names.iter().zip(per_criterion_len) {
            check_signed(&format!("evaluations para \"{name}\""), map.get(name), len)?;
        }
        Ok(())
    }

    /// Number of quiz items for the criteria quiz and for each criterion's quiz.
    fn quiz_sizes(&self, method: Method) -> (usize, Vec<usize>) {
        let pairs = |n: usize| n * n.saturating_sub(1) / 2;
        let per_criterion = self
            .criteria
            .iter()
            .map(|c| match method {
                Method::Classic => pairs(self.alternatives.len()),
                Method::sortII => pairs(RANGE_POINTS + c.profiles.len()),
            })
            .collect();
        (pairs(self.criteria.len()), per_criterion)
    }

    fn table(&self) -> Table {
        self.criteria
            .iter()
            .map(|c| (c.unit.clone(), c.values.clone()))
            .collect()
    }

    fn build_classic(&self) -> Result<Ahp, ProjectError> {
        self.validate_classic()?;
        Ok(Ahp::new(
            self.criteria_names(),
            self.alternatives.clone(),
            self.table(),
        )?)
    }

    fn build_sort(&self) -> Result<Ahpsort, ProjectError> {
        self.validate_sort()?;
        let cats = self.categories.as_ref().expect("validated");
        let profiles = (0..self.expected_profiles().unwrap_or(0))
            .map(|k| {
                let values = self
                    .criteria
                    .iter()
                    .map(|c| Value {
                        data: c.profiles[k],
                        unit: c.unit.clone(),
                    })
                    .collect();
                match cats.profile_type {
                    ProfileType::Central => Profile::Central(values),
                    ProfileType::Limiting => Profile::Limiting(values),
                }
            })
            .collect();
        let ranges = (0..self.criteria.len())
            .map(|i| self.effective_range(i))
            .collect();
        Ok(Ahpsort::new(
            self.criteria_names(),
            self.alternatives.clone(),
            self.table(),
            cats.names.clone(),
            profiles,
            ranges,
        )?)
    }
}

fn unique_names(names: &[String], what: &str) -> Result<(), ProjectError> {
    let mut seen = HashSet::new();
    for n in names {
        if n.trim().is_empty() {
            return Err(invalid(format!("hay una {what} sin nombre")));
        }
        if !seen.insert(n.as_str()) {
            return Err(invalid(format!("{what} repetida: \"{n}\"")));
        }
    }
    Ok(())
}

fn check_signed(what: &str, values: Option<&[f64]>, len: usize) -> Result<(), ProjectError> {
    let Some(values) = values else {
        return Ok(());
    };
    if values.len() != len {
        return Err(invalid(format!(
            "{what} tiene {} valores y debe tener {len}",
            values.len()
        )));
    }
    if values.iter().any(|v| !v.is_finite() || v.abs() < 1.0) {
        return Err(invalid(format!("{what}: cada valor debe cumplir |x| >= 1")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Quizzes and evaluations
// ---------------------------------------------------------------------------------------------

/// Criteria quiz and one alternatives quiz per criterion (classic AHP).
pub fn classic_quizzes(project: &Project) -> Result<QuizSet, ProjectError> {
    let ahp = project.build_classic()?;
    Ok((ahp.evaluate_criteria(), ahp.evaluate_alternatives()))
}

/// Criteria quiz and one range-points quiz per criterion (AHPSort II), exactly as
/// [`Ahpsort::evaluate_ranges`] produces them (labels, reference values and order).
pub fn sort_quizzes(project: &Project) -> Result<QuizSet, ProjectError> {
    let mut ahp = project.build_sort()?;
    Ok((ahp.evaluate_criteria(), ahp.evaluate_ranges()))
}

/// Quizzes for `method` (see [`classic_quizzes`] and [`sort_quizzes`]).
pub fn quizzes(project: &Project, method: Method) -> Result<QuizSet, ProjectError> {
    match method {
        Method::Classic => classic_quizzes(project),
        Method::sortII => sort_quizzes(project),
    }
}

/// Evaluation state of a project: signed Saaty values in quiz order.
///
/// `per_criterion[i]` holds the quiz of criterion `i`: alternatives (classic) or range points
/// (AHPSort II).
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluations {
    pub criteria: Vec<f64>,
    pub per_criterion: Vec<Vec<f64>>,
}

/// Which parts of the evaluations were absent from the file (`true` = missing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingEvaluations {
    pub criteria: bool,
    pub per_criterion: Vec<bool>,
}

impl MissingEvaluations {
    pub fn is_complete(&self) -> bool {
        !self.criteria && !self.per_criterion.iter().any(|&m| m)
    }

    pub fn is_empty_file(&self) -> bool {
        self.criteria && self.per_criterion.iter().all(|&m| m)
    }
}

impl Evaluations {
    /// Every comparison set to `1` (equal) for `method`.
    pub fn defaults(project: &Project, method: Method) -> Self {
        let (criteria, per_criterion) = project.quiz_sizes(method);
        Self {
            criteria: vec![1.0; criteria],
            per_criterion: per_criterion.into_iter().map(|n| vec![1.0; n]).collect(),
        }
    }

    /// Evaluations stored in the project file for `method`, with missing parts filled with
    /// `1` (equal), plus which parts were missing. Validates the project first.
    pub fn from_project(
        project: &Project,
        method: Method,
    ) -> Result<(Self, MissingEvaluations), ProjectError> {
        project.validate(method)?;
        let mut ev = Self::defaults(project, method);
        let file = &project.evaluations;
        let map = match method {
            Method::Classic => &file.alternatives,
            Method::sortII => &file.ranges,
        };

        let criteria_missing = match &file.criteria {
            Some(v) => {
                ev.criteria = v.clone();
                false
            }
            None => true,
        };
        let per_criterion = project
            .criteria
            .iter()
            .zip(ev.per_criterion.iter_mut())
            .map(|(c, slot)| match map.get(&c.name) {
                Some(v) => {
                    *slot = v.to_vec();
                    false
                }
                None => true,
            })
            .collect();

        Ok((
            ev,
            MissingEvaluations {
                criteria: criteria_missing,
                per_criterion,
            },
        ))
    }

    /// Writes these evaluations into `project.evaluations` (keyed by criterion name, in
    /// criterion order), replacing the part used by `method` and keeping the other one.
    pub fn store_into(&self, project: &mut Project, method: Method) {
        let named = NamedVectors(
            project
                .criteria
                .iter()
                .zip(&self.per_criterion)
                .map(|(c, v)| (c.name.clone(), v.clone()))
                .collect(),
        );
        project.evaluations.criteria = Some(self.criteria.clone());
        match method {
            Method::Classic => project.evaluations.alternatives = named,
            Method::sortII => project.evaluations.ranges = named,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Solve
// ---------------------------------------------------------------------------------------------

/// A full `n x n` pairwise matrix with its labels and weights.
#[derive(Debug, Clone)]
pub struct MatrixView {
    pub labels: Vec<String>,
    pub cells: Vec<Vec<f64>>,
    pub report: WeightsReport,
}

#[derive(Debug, Clone)]
pub struct ClassicOutcome {
    pub criteria: MatrixView,
    /// One alternatives matrix per criterion: `(criterion, matrix)`.
    pub alternatives: Vec<(String, MatrixView)>,
    /// `(alternative, global priority)`, sorted descending.
    pub ranking: Vec<(String, f64)>,
}

/// A range point (RP) or category profile of an AHPSort II criterion.
#[derive(Debug, Clone, PartialEq)]
pub struct RangePoint {
    pub label: String,
    pub original: f64,
    pub weight: f64,
    /// Category index when the point is a profile.
    pub profile: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct sortOutcome {
    pub criteria: MatrixView,
    /// One range-points matrix per criterion: `(criterion, matrix)`.
    pub ranges: Vec<(String, MatrixView)>,
    /// Points of each criterion `(criterion, points)`, sorted ascending by `original`.
    pub local_priorities: Vec<(String, Vec<RangePoint>)>,
    /// `(alternative, local priorities in criterion order, global priority)`.
    pub alternatives: Vec<(String, Vec<f64>, f64)>,
    /// `(category, local priorities in criterion order, global priority)` for each profile.
    pub profiles: Vec<(String, Vec<f64>, f64)>,
    /// `(alternative, global priority, category)`.
    pub classification: Vec<(String, f64, String)>,
}

/// Quiz with the signed values applied (rounded) and the upper cells for the exact matrix.
fn signed_quiz(quiz: &mut QuizPairMatrix, signed: &[f64]) -> Result<UpperCells, ProjectError> {
    quiz.apply_signed(signed)?;
    let n = quiz.num_var();
    let cells = (0..n)
        .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
        .zip(signed)
        .map(|((i, j), &x)| (i, j, signed_to_ratio(x)))
        .collect();
    Ok(cells)
}

fn matrix_view(
    labels: Vec<String>,
    get: impl Fn(usize, usize) -> Option<f64>,
    report: Option<WeightsReport>,
) -> Result<MatrixView, ProjectError> {
    let n = labels.len();
    let cells = (0..n)
        .map(|i| (0..n).map(|j| get(i, j).unwrap_or(1.0)).collect())
        .collect();
    Ok(MatrixView {
        labels,
        cells,
        report: report.ok_or(ProjectError::Library("[project] Falta una matriz"))?,
    })
}

fn check_evaluations(
    ev: &Evaluations,
    criteria: &QuizPairMatrix,
    per_criterion: &[(String, QuizPairMatrix)],
) -> Result<(), ProjectError> {
    if ev.criteria.len() != criteria.scores().len() || ev.per_criterion.len() != per_criterion.len()
    {
        return Err(invalid(
            "las evaluaciones no corresponden a los cuestionarios del proyecto",
        ));
    }
    for ((name, quiz), v) in per_criterion.iter().zip(&ev.per_criterion) {
        if quiz.scores().len() != v.len() {
            return Err(invalid(format!(
                "las evaluaciones de \"{name}\" tienen {} valores y deben tener {}",
                v.len(),
                quiz.scores().len()
            )));
        }
    }
    // Values must be valid signed Saaty values (finite, |x| >= 1): anything else would be
    // silently turned into "equal" or produce NaN weights.
    check_signed(
        "las evaluaciones de los criterios",
        Some(&ev.criteria),
        ev.criteria.len(),
    )?;
    for ((name, _), v) in per_criterion.iter().zip(&ev.per_criterion) {
        check_signed(&format!("las evaluaciones de \"{name}\""), Some(v), v.len())?;
    }
    Ok(())
}

/// Solves a classic AHP project with the exact signed evaluations, calculating every weight
/// (criteria, alternatives and ranking) with `method`.
pub fn solve_classic(
    project: &Project,
    ev: &Evaluations,
    method: WeightMethod,
) -> Result<ClassicOutcome, ProjectError> {
    let mut ahp = project.build_classic()?;
    ahp.set_weight_method(method);
    let mut c_quiz = ahp.evaluate_criteria();
    let a_quizzes = ahp.evaluate_alternatives();
    check_evaluations(ev, &c_quiz, &a_quizzes)?;

    let c_labels = c_quiz.labels();
    let c_cells = signed_quiz(&mut c_quiz, &ev.criteria)?;
    ahp.submit_criteria(c_quiz)?;
    for (i, j, r) in c_cells {
        ahp.set_matrix_criterion_value(i, j, r)?;
    }

    let mut quizzes = Vec::new();
    let mut cells = Vec::new();
    for ((_, mut q), v) in a_quizzes.into_iter().zip(&ev.per_criterion) {
        let c = signed_quiz(&mut q, v)?;
        quizzes.push(q);
        cells.push(c);
    }
    ahp.submit_alternatives(quizzes)?;
    for (k, cs) in cells.into_iter().enumerate() {
        for (i, j, r) in cs {
            ahp.set_matrix_alternative_value(k, i, j, r)?;
        }
    }

    let criteria = matrix_view(
        c_labels,
        |i, j| ahp.get_matrix_criterion_value(i, j),
        ahp.criteria_weights(),
    )?;
    let alternatives = project
        .criteria
        .iter()
        .enumerate()
        .map(|(k, c)| {
            matrix_view(
                project.alternatives.clone(),
                |i, j| ahp.get_matrix_alternative_value(k, i, j),
                ahp.alternative_weights(k),
            )
            .map(|m| (c.name.clone(), m))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut ranking = ahp.rank()?;
    ranking.sort_by(|a, b| b.1.total_cmp(&a.1));

    Ok(ClassicOutcome {
        criteria,
        alternatives,
        ranking,
    })
}

/// Solves an AHPSort II project with the exact signed evaluations, calculating every weight
/// (criteria, ranges and classification) with `method`.
pub fn solve_sort(
    project: &Project,
    ev: &Evaluations,
    method: WeightMethod,
) -> Result<sortOutcome, ProjectError> {
    let mut ahp = project.build_sort()?;
    ahp.set_weight_method(method);
    let mut c_quiz = ahp.evaluate_criteria();
    let r_quizzes = ahp.evaluate_ranges();
    check_evaluations(ev, &c_quiz, &r_quizzes)?;

    let c_labels = c_quiz.labels();
    let c_cells = signed_quiz(&mut c_quiz, &ev.criteria)?;
    ahp.submit_criteria(c_quiz)?;
    for (i, j, r) in c_cells {
        ahp.set_matrix_criterion_value(i, j, r)?;
    }

    let mut r_labels = Vec::new();
    let mut quizzes = Vec::new();
    let mut cells = Vec::new();
    for ((name, mut q), v) in r_quizzes.into_iter().zip(&ev.per_criterion) {
        r_labels.push((name, q.labels()));
        let c = signed_quiz(&mut q, v)?;
        quizzes.push(q);
        cells.push(c);
    }
    ahp.submit_ranges(quizzes)?;
    // `set_matrix_range_value` also refreshes the range priorities
    for (k, cs) in cells.into_iter().enumerate() {
        for (i, j, r) in cs {
            ahp.set_matrix_range_value(k, i, j, r)?;
        }
    }

    let criteria = matrix_view(
        c_labels,
        |i, j| ahp.get_matrix_criterion_value(i, j),
        ahp.criteria_weights(),
    )?;
    let ranges = r_labels
        .iter()
        .enumerate()
        .map(|(k, (name, labels))| {
            matrix_view(
                labels.to_owned(),
                |i, j| ahp.get_matrix_range_value(k, i, j),
                ahp.ranges_weights(k),
            )
            .map(|m| (name.to_owned(), m))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let local_priorities = ahp
        .ranges_priorities()
        .ok_or(ProjectError::Library(
            "[project] Faltan las prioridades de rangos",
        ))?
        .into_iter()
        .zip(&r_labels)
        .map(|((name, prios), (_, labels))| {
            let mut points = prios
                .iter()
                .zip(labels)
                .map(|(p, label)| RangePoint {
                    label: label.clone(),
                    original: p.original,
                    weight: p.weight.unwrap_or(0.0),
                    profile: p.profile,
                })
                .collect::<Vec<_>>();
            points.sort_by(|a, b| a.original.total_cmp(&b.original));
            (name.to_owned(), points)
        })
        .collect();

    let owned = |rows: PriorityRows| {
        rows.into_iter()
            .map(|(label, local, global)| {
                (
                    label.to_owned(),
                    local.into_iter().map(|(_, p)| p).collect(),
                    global,
                )
            })
            .collect::<Vec<_>>()
    };
    let alternatives = owned(ahp.alternatives_global_priorities()?);
    let profiles = owned(ahp.profiles_global_priorities()?);
    let classification = ahp.classify()?;

    Ok(sortOutcome {
        criteria,
        ranges,
        local_priorities,
        alternatives,
        profiles,
        classification,
    })
}

use super::*;

fn example(name: &str) -> Project {
    let path = format!("{}/../examples/{name}", env!("CARGO_MANIFEST_DIR"));
    Project::load(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn assert_close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "expected {b}, got {a}");
}

/// Range evaluations where a lower value is preferred (as in the London example).
fn lower_is_better(project: &Project) -> Vec<Vec<f64>> {
    let (_, quizzes) = sort_quizzes(project).unwrap();
    quizzes
        .iter()
        .enumerate()
        .map(|(k, (_, q))| {
            let range = project.effective_range(k);
            let d = |v: f64| 9.0 - 8.0 * v / range;
            q.scores()
                .iter()
                .map(|it| {
                    let r = d(it.ref_value1.as_ref().unwrap().data)
                        / d(it.ref_value2.as_ref().unwrap().data);
                    ratio_to_signed_rounded(r)
                })
                .collect()
        })
        .collect()
}

fn ratio_to_signed_rounded(r: f64) -> f64 {
    let s = if r >= 1.0 {
        r.round()
    } else {
        -(1.0 / r).round()
    };
    if s.abs() <= 1.0 { 1.0 } else { s }
}

#[test]
fn every_example_round_trips_through_toml() {
    for name in [
        "coches.ahp",
        "coches_sin_evaluar.ahp",
        "londres.ahp",
        "londres_parcial.ahp",
    ] {
        let p = example(name);
        let text = p.to_toml_string().unwrap();
        assert_eq!(Project::from_toml_str(&text).unwrap(), p, "{name}");
    }
}

#[test]
fn saved_files_skip_empty_fields() {
    let text = example("coches_sin_evaluar.ahp").to_toml_string().unwrap();
    for absent in ["range", "profiles", "categories", "evaluations"] {
        assert!(!text.contains(absent), "unexpected `{absent}` in:\n{text}");
    }
}

#[test]
fn parse_errors_are_readable() {
    let err = Project::from_toml_str("alternatives = [\"a\"]\nfoo = 1\n").unwrap_err();
    assert!(matches!(err, ProjectError::Parse(_)));
    assert!(err.to_string().contains("foo"), "{err}");
    let err = Project::from_toml_str("method = \"other\"").unwrap_err();
    assert!(matches!(err, ProjectError::Parse(_)));
}

#[test]
fn effective_range_defaults_to_max_value() {
    let mut p = example("coches_sin_evaluar.ahp");
    assert_eq!(p.effective_range(0), 31000.0);
    p.criteria[0].range = Some(50000.0);
    assert_eq!(p.effective_range(0), 50000.0);
    p.criteria[1].values.clear();
    assert_eq!(p.effective_range(1), 0.0);
    assert_eq!(p.effective_range(99), 0.0);
}

#[test]
fn solves_the_classic_example() {
    let p = example("coches.ahp");
    let (ev, missing) = Evaluations::from_project(&p, Method::Classic).unwrap();
    assert!(missing.is_complete());

    let out = solve_classic(&p, &ev, WeightMethod::EigenValues).unwrap();
    assert_eq!(out.ranking.len(), 4);
    assert!(
        out.ranking.windows(2).all(|w| w[0].1 >= w[1].1),
        "descending"
    );
    assert_close(out.ranking.iter().map(|r| r.1).sum(), 1.0, 1e-9);
    assert_close(out.criteria.report.weights.iter().sum(), 1.0, 1e-9);
    assert_eq!(out.criteria.labels, p.criteria_names());
    assert_eq!(out.alternatives.len(), 4);
    for (_, m) in &out.alternatives {
        assert_eq!(m.labels, p.alternatives);
        assert_eq!(m.cells.len(), 4);
        assert_close(m.report.weights.iter().sum(), 1.0, 1e-9);
    }
    // Precio-Consumo = 3 => a_01 = 3, a_10 = 1/3
    assert_close(out.criteria.cells[0][1], 3.0, 1e-12);
    assert_close(out.criteria.cells[1][0], 1.0 / 3.0, 1e-12);
}

#[test]
fn solve_uses_the_exact_signed_values() {
    let p = example("coches.ahp");
    let (mut ev, _) = Evaluations::from_project(&p, Method::Classic).unwrap();
    ev.criteria[0] = 2.5;
    ev.per_criterion[0][0] = -3.5;
    let out = solve_classic(&p, &ev, WeightMethod::EigenValues).unwrap();
    assert_close(out.criteria.cells[0][1], 2.5, 1e-12);
    assert_close(out.alternatives[0].1.cells[0][1], 1.0 / 3.5, 1e-12);
    assert_close(out.alternatives[0].1.cells[1][0], 3.5, 1e-12);
}

#[test]
fn solves_the_sort_example_with_a_mix_of_categories() {
    let p = example("londres.ahp");
    let (ev, missing) = Evaluations::from_project(&p, Method::sortII).unwrap();
    assert!(missing.is_complete());

    let out = solve_sort(&p, &ev, WeightMethod::EigenValues).unwrap();
    let names = &p.categories.as_ref().unwrap().names;
    assert_eq!(out.classification.len(), p.alternatives.len());
    assert!(out.classification.iter().all(|(_, _, c)| names.contains(c)));
    let distinct: HashSet<&str> = out
        .classification
        .iter()
        .map(|(_, _, c)| c.as_str())
        .collect();
    assert!(distinct.len() >= 2, "{:?}", out.classification);

    assert_eq!(out.profiles.len(), 3);
    assert_eq!(out.alternatives.len(), 6);
    assert!(
        out.alternatives
            .iter()
            .all(|(_, local, _)| local.len() == 5)
    );
    for (_, points) in &out.local_priorities {
        assert_eq!(points.len(), 9);
        assert!(points.windows(2).all(|w| w[0].original <= w[1].original));
        assert_eq!(points.iter().filter(|p| p.profile.is_some()).count(), 3);
        assert_eq!(points[0].label, "RP1");
    }
    // The point weights are the eigenvector of the range matrix
    let (_, points) = &out.local_priorities[0];
    let (_, matrix) = &out.ranges[0];
    for point in points {
        let idx = matrix
            .labels
            .iter()
            .position(|l| *l == point.label)
            .unwrap();
        assert_close(point.weight, matrix.report.weights[idx], 1e-12);
    }
}

#[test]
fn quiz_sizes_match_the_library_quizzes() {
    let p = example("londres.ahp");
    let (c, r) = sort_quizzes(&p).unwrap();
    let (cs, rs) = p.quiz_sizes(Method::sortII);
    assert_eq!(c.scores().len(), cs);
    assert_eq!(
        r.iter().map(|(_, q)| q.scores().len()).collect::<Vec<_>>(),
        rs
    );

    let p = example("coches.ahp");
    let (c, a) = classic_quizzes(&p).unwrap();
    let (cs, as_) = p.quiz_sizes(Method::Classic);
    assert_eq!(c.scores().len(), cs);
    assert_eq!(
        a.iter().map(|(_, q)| q.scores().len()).collect::<Vec<_>>(),
        as_
    );
}

#[test]
fn detects_missing_parts_of_partial_files() {
    let (ev, missing) =
        Evaluations::from_project(&example("coches_sin_evaluar.ahp"), Method::Classic).unwrap();
    assert!(missing.is_empty_file());
    assert_eq!(
        ev,
        Evaluations::defaults(&example("coches.ahp"), Method::Classic)
    );

    let p = example("londres_parcial.ahp");
    let (ev, missing) = Evaluations::from_project(&p, Method::sortII).unwrap();
    assert!(!missing.criteria);
    assert_eq!(missing.per_criterion, vec![true; 5]);
    assert_eq!(&ev.criteria, p.evaluations.criteria.as_ref().unwrap());
    assert!(ev.per_criterion.iter().flatten().all(|&x| x == 1.0));
}

#[test]
fn stored_evaluations_reload_complete() {
    let mut p = example("londres_parcial.ahp");
    let (mut ev, _) = Evaluations::from_project(&p, Method::sortII).unwrap();
    ev.per_criterion = lower_is_better(&p);
    ev.store_into(&mut p, Method::sortII);

    let reloaded = Project::from_toml_str(&p.to_toml_string().unwrap()).unwrap();
    let (again, missing) = Evaluations::from_project(&reloaded, Method::sortII).unwrap();
    assert!(missing.is_complete());
    assert_eq!(again, ev);
    // Keyed by criterion name, in criterion order
    let keys: Vec<&str> = reloaded
        .evaluations
        .ranges
        .0
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(keys, p.criteria_names());
    // Same results as the shipped complete example
    assert_eq!(
        solve_sort(&reloaded, &again, WeightMethod::EigenValues)
            .unwrap()
            .classification,
        {
            let full = example("londres.ahp");
            let (ev, _) = Evaluations::from_project(&full, Method::sortII).unwrap();
            solve_sort(&full, &ev, WeightMethod::EigenValues)
                .unwrap()
                .classification
        }
    );
}

#[test]
fn limiting_profiles_use_the_last_category_as_catch_all() {
    let text = r#"
        alternatives = ["bueno", "medio", "malo"]

        [[criteria]]
        name = "c1"
        values = [10.0, 50.0, 95.0]
        range = 100.0
        profiles = [30.0, 70.0]

        [[criteria]]
        name = "c2"
        values = [5.0, 45.0, 90.0]
        range = 100.0
        profiles = [30.0, 70.0]

        [categories]
        profile_type = "limiting"
        names = ["A", "B", "C"]
    "#;
    let p = Project::from_toml_str(text).unwrap();
    let (mut ev, _) = Evaluations::from_project(&p, Method::sortII).unwrap();
    ev.per_criterion = lower_is_better(&p);
    let out = solve_sort(&p, &ev, WeightMethod::EigenValues).unwrap();

    assert_eq!(out.profiles.len(), 2, "the last category has no profile");
    let class = |alt: &str| {
        out.classification
            .iter()
            .find(|(a, _, _)| a == alt)
            .map(|(_, _, c)| c.as_str())
            .unwrap()
    };
    assert!(
        out.classification
            .iter()
            .all(|(_, _, c)| ["A", "B", "C"].contains(&c.as_str()))
    );
    // Lower values are preferred, so profile A (30) outranks profile B (70). Categories are
    // bounded by ascending profile priority: below B's limit => B, up to A's limit => A,
    // and the catch-all C collects the alternative above every limit.
    assert_eq!(class("malo"), "B");
    assert_eq!(class("medio"), "A");
    assert_eq!(class("bueno"), "C");
}

#[test]
fn validation_rejects_inconsistent_projects() {
    let base = example("londres.ahp");
    let expect_invalid = |p: &Project, method: Method, needle: &str| {
        let err = p.validate(method).unwrap_err();
        assert!(matches!(err, ProjectError::Invalid(_)), "{err}");
        assert!(
            err.to_string().contains(needle),
            "`{needle}` not in `{err}`"
        );
    };

    let mut p = base.clone();
    p.criteria[0].values.pop();
    expect_invalid(&p, Method::sortII, "valores");

    let mut p = base.clone();
    p.alternatives[1] = p.alternatives[0].clone();
    expect_invalid(&p, Method::sortII, "repetida");

    let mut p = base.clone();
    p.criteria[2].profiles.pop();
    expect_invalid(&p, Method::sortII, "perfiles");

    let mut p = base.clone();
    p.categories.as_mut().unwrap().profile_type = ProfileType::Limiting;
    expect_invalid(&p, Method::sortII, "perfiles");

    let mut p = base.clone();
    p.categories = None;
    expect_invalid(&p, Method::sortII, "categories");

    let mut p = base.clone();
    p.evaluations.ranges.0[0].1.pop();
    expect_invalid(&p, Method::sortII, "debe tener");

    let mut p = base.clone();
    p.evaluations.ranges.0[0].0 = "No existe".into();
    expect_invalid(&p, Method::sortII, "No existe");

    let mut p = base.clone();
    p.evaluations.criteria.as_mut().unwrap()[0] = 0.5;
    expect_invalid(&p, Method::sortII, "|x| >= 1");

    let mut p = example("coches.ahp");
    p.criteria.truncate(1);
    p.evaluations = FileEvaluations::default();
    expect_invalid(&p, Method::Classic, "2 criterios");

    assert!(example("coches.ahp").validate_classic().is_ok());
    assert!(base.validate_sort().is_ok());
}

#[test]
fn solve_rejects_evaluations_of_the_wrong_shape() {
    let p = example("coches.ahp");
    let mut ev = Evaluations::defaults(&p, Method::Classic);
    ev.per_criterion[1].pop();
    assert!(matches!(
        solve_classic(&p, &ev, WeightMethod::EigenValues),
        Err(ProjectError::Invalid(_))
    ));
}

#[test]
fn solve_rejects_non_finite_or_out_of_scale_evaluations() {
    let p = example("coches.ahp");
    let (ev, _) = Evaluations::from_project(&p, Method::Classic).unwrap();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, 0.5, -0.99] {
        let mut criteria_bad = ev.clone();
        criteria_bad.criteria[0] = bad;
        assert!(
            matches!(
                solve_classic(&p, &criteria_bad, WeightMethod::EigenValues),
                Err(ProjectError::Invalid(_))
            ),
            "criteria value {bad} must be rejected"
        );
        let mut alt_bad = ev.clone();
        alt_bad.per_criterion[2][1] = bad;
        assert!(
            matches!(
                solve_classic(&p, &alt_bad, WeightMethod::EigenValues),
                Err(ProjectError::Invalid(_))
            ),
            "alternative value {bad} must be rejected"
        );
    }

    let sort = example("londres.ahp");
    let mut ev = Evaluations::defaults(&sort, Method::sortII);
    ev.per_criterion[0][0] = f64::NAN;
    assert!(matches!(
        solve_sort(&sort, &ev, WeightMethod::EigenValues),
        Err(ProjectError::Invalid(_))
    ));
    let (valid, _) = Evaluations::from_project(&p, Method::Classic).unwrap();
    assert!(solve_classic(&p, &valid, WeightMethod::EigenValues).is_ok());
}
