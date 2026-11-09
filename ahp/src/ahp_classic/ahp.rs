use crate::share::{
    pair_matrix::PairMatrix,
    pesos::{WeightMethod, WeightsReport},
    quiz::QuizPairMatrix,
    share::Table,
};

pub struct Ahp {
    criteria_labels: Vec<String>,
    alternatives_labels: Vec<String>,
    values: Table, // Criterios x Alternativas

    criteria_matrix: Option<PairMatrix>,
    alt_matrices: Option<Vec<PairMatrix>>,
}

impl Ahp {
    pub fn new(
        c_labels: Vec<impl ToString>,
        alt_labels: Vec<impl ToString>,
        values: Table,
    ) -> Result<Self, &'static str> {
        // Each row of `values` is a criterion with one value per alternative
        if c_labels.len() != values.len()
            || values.iter().any(|(_, row)| row.len() != alt_labels.len())
        {
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

    pub fn criteria_labels(&self) -> &[String] {
        &self.criteria_labels
    }

    pub fn alternatives_labels(&self) -> &[String] {
        &self.alternatives_labels
    }

    /// Values table: one row per criterion, one column per alternative.
    pub fn values(&self) -> &Table {
        &self.values
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
        } else if quizs
            .iter()
            .any(|q| q.num_var() != self.alternatives_labels.len())
        {
            Err(
                "[Ahp::sumbit_alternatives] Alguno de los cuestionarios proporcionados no evalua el mismo número de alternativas que el sistema.",
            )
        } else {
            self.alt_matrices = Some(quizs.into_iter().map(PairMatrix::from_quiz).collect());
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

        ranking.sort_by(|a, b| a.1.total_cmp(&b.1));
        Ok(ranking)
    }
}

#[cfg(test)]
mod tests {
    use super::Ahp;

    fn table(rows: &[&[f64]]) -> crate::share::share::Table {
        rows.iter().map(|r| (String::new(), r.to_vec())).collect()
    }

    #[test]
    fn new_accepts_criteria_rows_and_alternative_columns() {
        // 3 criteria x 2 alternatives: rejected before the fix (lengths were compared swapped)
        let ahp = Ahp::new(
            vec!["c1", "c2", "c3"],
            vec!["a1", "a2"],
            table(&[&[1.0, 2.0], &[3.0, 4.0], &[5.0, 6.0]]),
        );
        assert!(ahp.is_ok());
        let ahp = ahp.unwrap();
        assert_eq!(ahp.criteria_labels().len(), 3);
        assert_eq!(ahp.alternatives_labels().len(), 2);
        assert_eq!(ahp.values().len(), 3);
    }

    #[test]
    fn new_rejects_mismatched_tables_without_panicking() {
        // Swapped table (alternatives as rows)
        assert!(
            Ahp::new(
                vec!["c1", "c2", "c3"],
                vec!["a1", "a2"],
                table(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]),
            )
            .is_err()
        );
        // A ragged row
        assert!(
            Ahp::new(
                vec!["c1", "c2"],
                vec!["a1", "a2"],
                table(&[&[1.0, 2.0], &[3.0]])
            )
            .is_err()
        );
        // Empty values with labels: used to panic on `values[0]`
        assert!(Ahp::new(vec!["c1"], vec!["a1"], table(&[])).is_err());
    }
}
