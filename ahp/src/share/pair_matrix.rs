use crate::share::quiz::{AhpScore, QuizPairMatrix};

#[derive(Debug, Clone)]
pub struct PairMatrix {
    size: usize,
    data: Vec<f64>,
}

impl PairMatrix {
    pub fn from_quiz(quiz: QuizPairMatrix) -> Self {
        let n = quiz.num_var();
        let data = quiz
            .items
            .into_iter()
            .map(|item| match item.score {
                AhpScore::LeftDominates(s) => s as u8 as f64,
                AhpScore::Equal => 1.0,
                AhpScore::RightDominates(s) => 1.0 / s as u8 as f64,
            })
            .collect();

        Self { size: n, data }
    }

    fn map_idx(&self, i: usize, j: usize) -> usize {
        i * self.size - (i * (i + 1)) / 2 + (j - i - 1)
    }

    pub fn get(&self, row: usize, col: usize) -> Option<f64> {
        if row >= self.size || col >= self.size {
            return None; // Fuera de rango
        }

        if row == col {
            Some(1.0)
        } else if row < col {
            // diagonal superior
            let idx = self.map_idx(row, col);
            Some(self.data[idx])
        } else {
            let idx = self.map_idx(col, row);
            Some(1.0 / self.data[idx])
        }
    }

    pub fn get_size(&self) -> usize {
        self.size
    }

    pub fn set(&mut self, row: usize, col: usize, val: f64) -> Result<(), &'static str> {
        if row >= self.size || col >= self.size {
            return Err("[PairMatrix::set] Índices fuera de rango");
        }
        if row == col {
            return Err(
                "[PairMatrix::set] No se puede modificar la diagonal principal (siempre es 1.0)",
            );
        }
        if val == 0.0 {
            return Err("[PairMatrix::set] El valor en una matriz recíproca no puede ser 0");
        }

        if row < col {
            let idx = self.map_idx(row, col);
            self.data[idx] = val;
        } else {
            let idx = self.map_idx(col, row);
            self.data[idx] = 1.0 / val;
        }
        Ok(())
    }
}
