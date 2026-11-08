use crate::share::{
    pair_matrix::PairMatrix,
    pesos::{WeightMethod, WeightsReport},
    quiz::QuizPairMatrix,
    share::Table,
};

pub struct Ahp {
    criteria_labels: Vec<String>,
    alternatives_labels: Vec<String>,
    values: Table,

    criteria_matrix: Option<PairMatrix>,
    alt_matrices: Option<Vec<PairMatrix>>,
}

impl Ahp {
    pub fn new(
        c_labels: Vec<impl ToString>,
        alt_labels: Vec<impl ToString>,
        values: Table,
    ) -> Result<Self, &'static str> {
        if alt_labels.len() != values.len() || c_labels.len() != values[0].1.len() {
            Err(
                "[Ahp::new] La cardinalidad de la matriz de valores no coincide con los vectores de nombes, asegurate de que los criterios son las filas y las alternativas las columnas.",
            )
        } else {
            Ok(Self {
                criteria_labels: c_labels.into_iter().map(|l| l.to_string()).collect(),
                alternatives_labels: alt_labels.into_iter().map(|l| l.to_string()).collect(),
                values,
                criteria_matrix: None,
                alt_matrices: None,
            })
        }
    }

    pub fn evaluate_criteria(&self) -> QuizPairMatrix {
        QuizPairMatrix::new(&self.criteria_labels, None)
    }

    pub fn evaluate_alternatives(&self) -> Vec<(String, QuizPairMatrix)> {
        self.criteria_labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                (
                    l.to_owned(),
                    QuizPairMatrix::new(&self.alternatives_labels, Some(&self.values[i])),
                )
            })
            .collect()
    }

    pub fn submit_criteria(&mut self, quiz: QuizPairMatrix) -> Result<(), &'static str> {
        if quiz.num_var() != self.criteria_labels.len() {
            Err(
                "[Ahp::sumbit_criteria] El cuestionario proporcionado no evalua el mismo número de criterios que el sistema.",
            )
        } else {
            self.criteria_matrix = Some(PairMatrix::from_quiz(quiz));
            Ok(())
        }
    }

    pub fn submit_alternatives(&mut self, quizs: Vec<QuizPairMatrix>) -> Result<(), &'static str> {
        if quizs.len() != self.criteria_labels.len() {
            Err("[Ahp::sumbit_alternatives] Hay más cuestionarios que criterios en el sistema.")
        } else if quizs[0].num_var() != self.alternatives_labels.len() {
            Err(
                "[Ahp::sumbit_alternatives] Alguno de los cuestionarios proporcionados no evalua el mismo número de alternativas que el sistema.",
            )
        } else {
            self.alt_matrices = Some(
                quizs
                    .into_iter()
                    .map(|q| PairMatrix::from_quiz(q))
                    .collect(),
            );
            Ok(())
        }
    }

    pub fn get_matrix_criterion_value(&self, row: usize, col: usize) -> Option<f64> {
        self.criteria_matrix.as_ref().and_then(|m| m.get(row, col))
    }
    pub fn set_matrix_criterion_value(
        &mut self,
        row: usize,
        col: usize,
        value: f64,
    ) -> Result<(), &'static str> {
        self.criteria_matrix
            .as_mut()
            .ok_or("[Ahp::set_matrix_criterion_value] La matriz de criterios todavía no existe")?
            .set(row, col, value)
    }
    pub fn get_matrix_alternative_value(
        &self,
        criterion_idx: usize,
        row: usize,
        col: usize,
    ) -> Option<f64> {
        self.alt_matrices
            .as_ref()? // None si aún no se han creado
            .get(criterion_idx)? // None si el criterio no existe
            .get(row, col)
    }

    pub fn set_matrix_alternative_value(
        &mut self,
        criterion_idx: usize,
        row: usize,
        col: usize,
        value: f64,
    ) -> Result<(), &'static str> {
        self.alt_matrices
            .as_mut()
            .ok_or("[Ahp::set_matrix_alternative_value] Las matrices de alternativas todavía no existen")?
            .get_mut(criterion_idx)
            .ok_or("[Ahp::set_matrix_alternative_value] El criterio no existe")?
            .set(row, col, value)
    }

    pub fn criteria_weights(&self, method: WeightMethod) -> Option<WeightsReport> {
        self.criteria_matrix
            .as_ref()
            .map(|m| WeightsReport::from_matrix(method, m))
    }

    pub fn alternative_weights(
        &self,
        criterion_idx: usize,
        method: WeightMethod,
    ) -> Option<WeightsReport> {
        self.alt_matrices
            .as_ref()?
            .get(criterion_idx)
            .map(|m| WeightsReport::from_matrix(method, m))
    }

    pub fn rank(&self) -> Result<Vec<(String, f64)>, &'static str> {
        let c_matrix = self
            .criteria_matrix
            .as_ref()
            .ok_or("[Ahp::rank] Falta la matriz de criterios")?;
        let criteria_weights = WeightMethod::EigenValues.calc_weights(c_matrix);

        // Un vector de pesos por criterio, en el orden de criteria_labels
        let alt_matrices = self
            .alt_matrices
            .as_ref()
            .ok_or("[Ahp::rank] Falta alguna matriz de alternativas")?;
        let alternatives_weights = alt_matrices
            .iter()
            .map(|m| WeightMethod::EigenValues.calc_weights(m))
            .collect::<Vec<Vec<f64>>>();

        let mut ranking = self
            .alternatives_labels
            .iter()
            .enumerate()
            .map(|(a, label)| {
                let score = alternatives_weights
                    .iter()
                    .zip(&criteria_weights)
                    .map(|(wc, cw)| wc[a] * cw)
                    .sum::<f64>();
                (label.to_owned(), score)
            })
            .collect::<Vec<(String, f64)>>();

        ranking.sort_by(|a, b| b.1.total_cmp(&a.1));
        Ok(ranking)
    }
}
