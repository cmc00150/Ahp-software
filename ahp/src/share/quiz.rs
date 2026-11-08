use crate::share::share::{RowWithUnits, Value};

#[derive(Debug, Clone)]
pub struct QuizPairMatrix(pub(crate) Vec<QuizItem>);

#[derive(Debug, Clone)]
pub struct QuizItem {
    pub label1: String,
    pub ref_value1: Option<Value>,
    pub label2: String,
    pub ref_value2: Option<Value>,
    pub score: AhpScore,
}

impl QuizPairMatrix {
    pub fn new(labels: &[String], row: Option<&RowWithUnits>) -> Self {
        let n = labels.len();
        Self(
            (0..n)
                .flat_map(|i| {
                    (i + 1..n).map(move |j| QuizItem {
                        label1: labels[i].clone(),
                        ref_value1: row.and_then(|r| {
                            Some(Value {
                                data: r.1[i],
                                unit: r.0.to_owned(),
                            })
                        }),
                        label2: labels[j].clone(),
                        ref_value2: row.and_then(|r| {
                            Some(Value {
                                data: r.1[j],
                                unit: r.0.to_owned(),
                            })
                        }),
                        score: AhpScore::Equal,
                    })
                })
                .collect(),
        )
    }

    pub fn scores(&self) -> &[QuizItem] {
        &self.0
    }

    pub fn set_score(&mut self, idx: usize, score: AhpScore) -> Option<()> {
        self.0.get_mut(idx).and_then(|item| {
            item.score = score;
            Some(())
        })
    }

    pub fn num_var(&self) -> usize {
        let tam = self.0.len() as f64;
        (0.5 + (2.0 * tam + 0.25).sqrt()).round() as usize
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
