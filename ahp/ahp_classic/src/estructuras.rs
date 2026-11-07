use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct QuizPairMatrix {
    labels: Vec<Rc<str>>,
    data: Vec<QuizItem>,
}

#[derive(Debug, Clone)]
struct QuizItem {
    label1: Rc<str>,
    label2: Rc<str>,
    score: AhpScore,
}

impl QuizPairMatrix {
    pub fn new(raw_labels: Vec<String>) -> Self {
        let labels: Vec<Rc<str>> = raw_labels.into_iter().map(Rc::from).collect();
        let mut data: Vec<QuizItem> = Vec::new();
        let n = labels.len();

        for i in 0..n {
            for j in i + 1..n {
                data.push(QuizItem {
                    label1: Rc::clone(&labels[i]),
                    label2: Rc::clone(&labels[j]),
                    score: AhpScore::Equal,
                });
            }
        }

        Self { labels, data }
    }

    pub fn scores(&self) -> &[QuizItem] {
        &self.data
    }

    pub fn set_score(&mut self, idx: usize, score: AhpScore) -> Option<()> {
        self.data.get_mut(idx).and_then(|item| {
            item.score = score;
            Some(())
        })
    }
}

#[derive(Debug, Clone)]
pub enum AhpScore {
    LeftDominates(SaatyScore),
    Equal,
    RightDominates(SaatyScore),
}

#[derive(Debug, Clone)]
pub enum SaatyScore {
    EqualPlus = 2,
    SlightlyBetter = 3,
    SlightlyBetterPlus = 4,
    StronglyBetter = 5,
    StronglyBetterPlus = 6,
    VeryStronglyBetter = 7,
    VeryStronglyBetterPlus = 8,
    ExtremelyBetter = 9,
}

pub struct PairMatrix {
    labels: Vec<String>,
    size: usize,
    data: Vec<f64>,
}

impl PairMatrix {
    pub fn from_quiz(quiz: QuizPairMatrix) -> Self {
        let size = quiz.labels.len();
        let data = quiz
            .data
            .into_iter()
            .map(|item| match item.score {
                AhpScore::LeftDominates(s) => s as u8 as f64,
                AhpScore::Equal => 1.0,
                AhpScore::RightDominates(s) => 1.0 / s as u8 as f64,
            })
            .collect();
        let labels = quiz.labels.into_iter().map(|rc| rc.to_string()).collect();

        Self { labels, size, data }
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
