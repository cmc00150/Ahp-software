use crate::share::share::{RowWithUnits, Value};

/// Pairwise-comparison questionnaire over `n` variables.
///
/// Items follow the upper triangle in row order: `(0,1), (0,2), ..., (0,n-1), (1,2), ...`.
#[derive(Debug, Clone)]
pub struct QuizPairMatrix {
    pub(crate) items: Vec<QuizItem>,
    labels: Vec<String>,
}

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
        let items = (0..n)
            .flat_map(|i| {
                (i + 1..n).map(move |j| QuizItem {
                    label1: labels[i].clone(),
                    ref_value1: row.map(|r| Value {
                        data: r.1[i],
                        unit: r.0.to_owned(),
                    }),
                    label2: labels[j].clone(),
                    ref_value2: row.map(|r| Value {
                        data: r.1[j],
                        unit: r.0.to_owned(),
                    }),
                    score: AhpScore::Equal,
                })
            })
            .collect();
        Self {
            items,
            labels: labels.to_vec(),
        }
    }

    pub fn scores(&self) -> &[QuizItem] {
        &self.items
    }

    pub fn set_score(&mut self, idx: usize, score: AhpScore) -> Option<()> {
        self.items.get_mut(idx).map(|item| {
            item.score = score;
        })
    }

    pub fn num_var(&self) -> usize {
        self.labels.len()
    }

    /// The `n` compared variables, in matrix order.
    pub fn labels(&self) -> Vec<String> {
        self.labels.clone()
    }

    /// Sets every item's score from signed Saaty values (quiz order), rounding each one to
    /// the nearest Saaty integer with [`AhpScore::from_signed`].
    ///
    /// # Errors
    ///
    /// If `signed` does not have exactly one value per item.
    pub fn apply_signed(&mut self, signed: &[f64]) -> Result<(), &'static str> {
        if signed.len() != self.items.len() {
            return Err(
                "[QuizPairMatrix::apply_signed] El número de valores no coincide con el número de preguntas.",
            );
        }
        for (item, &x) in self.items.iter_mut().zip(signed) {
            item.score = AhpScore::from_signed(x);
        }
        Ok(())
    }

    /// Signed Saaty value of every item, in quiz order.
    pub fn to_signed(&self) -> Vec<f64> {
        self.items
            .iter()
            .map(|item| f64::from(item.score.to_signed()))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhpScore {
    LeftDominates(SaatyScore),
    Equal,
    RightDominates(SaatyScore),
}

impl AhpScore {
    /// Builds a score from a signed Saaty value: `|x|` is rounded to the nearest integer and
    /// clamped to `1..=9`; `1` means equal, a positive value means the left item dominates and
    /// a negative one that the right item dominates. `NaN` is treated as equal.
    pub fn from_signed(x: f64) -> AhpScore {
        if x.is_nan() {
            return AhpScore::Equal;
        }
        let level = x.abs().round().clamp(1.0, 9.0) as u8;
        match SaatyScore::try_from(level) {
            Ok(s) if x > 0.0 => AhpScore::LeftDominates(s),
            Ok(s) => AhpScore::RightDominates(s),
            Err(_) => AhpScore::Equal,
        }
    }

    /// Signed Saaty value: `Equal` => 1, `LeftDominates(s)` => s, `RightDominates(s)` => -s.
    pub fn to_signed(&self) -> i8 {
        match self {
            AhpScore::Equal => 1,
            AhpScore::LeftDominates(s) => *s as i8,
            AhpScore::RightDominates(s) => -(*s as i8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl TryFrom<u8> for SaatyScore {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            2 => SaatyScore::EqualPlus,
            3 => SaatyScore::SlightlyBetter,
            4 => SaatyScore::SlightlyBetterPlus,
            5 => SaatyScore::StronglyBetter,
            6 => SaatyScore::StronglyBetterPlus,
            7 => SaatyScore::VeryStronglyBetter,
            8 => SaatyScore::VeryStronglyBetterPlus,
            9 => SaatyScore::ExtremelyBetter,
            _ => return Err("[SaatyScore::try_from] La escala de Saaty va de 2 a 9."),
        })
    }
}

/// Matrix cell for a signed Saaty value: `x >= 1` => `x`, `x <= -1` => `1/|x|`.
/// Values with `|x| < 1` (and `NaN`) are treated as `1` (equal).
pub fn signed_to_ratio(x: f64) -> f64 {
    if x >= 1.0 {
        x
    } else if x <= -1.0 {
        1.0 / x.abs()
    } else {
        1.0
    }
}

/// Inverse of [`signed_to_ratio`]: `r >= 1` => `r`, `r < 1` => `-1/r`.
pub fn ratio_to_signed(r: f64) -> f64 {
    if r >= 1.0 { r } else { -1.0 / r }
}

/// Spanish label for a Saaty intensity level (`1..=9`); values outside the range are clamped.
pub fn saaty_label_es(level: u8) -> &'static str {
    match level.clamp(1, 9) {
        1 => "Igual",
        2 => "Igual+",
        3 => "Algo mejor",
        4 => "Algo mejor+",
        5 => "Mucho mejor",
        6 => "Mucho mejor+",
        7 => "Muchísimo mejor",
        8 => "Muchísimo mejor+",
        _ => "Extremadamente mejor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("v{i}")).collect()
    }

    #[test]
    fn from_signed_rounds_clamps_and_keeps_direction() {
        assert_eq!(AhpScore::from_signed(1.0), AhpScore::Equal);
        assert_eq!(AhpScore::from_signed(-1.0), AhpScore::Equal);
        assert_eq!(AhpScore::from_signed(0.3), AhpScore::Equal);
        assert_eq!(AhpScore::from_signed(f64::NAN), AhpScore::Equal);
        assert_eq!(
            AhpScore::from_signed(3.4),
            AhpScore::LeftDominates(SaatyScore::SlightlyBetter)
        );
        assert_eq!(
            AhpScore::from_signed(-4.6),
            AhpScore::RightDominates(SaatyScore::StronglyBetter)
        );
        assert_eq!(
            AhpScore::from_signed(42.0),
            AhpScore::LeftDominates(SaatyScore::ExtremelyBetter)
        );
    }

    #[test]
    fn to_signed_roundtrips_every_integer_level() {
        for x in (-9i8..=9).filter(|x| x.abs() >= 1) {
            let expected = if x.abs() == 1 { 1 } else { x };
            assert_eq!(AhpScore::from_signed(f64::from(x)).to_signed(), expected);
        }
    }

    #[test]
    fn saaty_try_from_accepts_only_two_to_nine() {
        assert!(SaatyScore::try_from(1).is_err());
        assert!(SaatyScore::try_from(10).is_err());
        assert_eq!(SaatyScore::try_from(7), Ok(SaatyScore::VeryStronglyBetter));
    }

    #[test]
    fn signed_and_ratio_conversions() {
        assert_eq!(signed_to_ratio(5.0), 5.0);
        assert_eq!(signed_to_ratio(-4.0), 0.25);
        assert_eq!(signed_to_ratio(0.5), 1.0);
        assert_eq!(signed_to_ratio(-1.0), 1.0);
        assert_eq!(ratio_to_signed(0.25), -4.0);
        assert_eq!(ratio_to_signed(2.5), 2.5);
        for x in [1.0, 2.5, 9.0, -2.0, -7.5] {
            assert!((ratio_to_signed(signed_to_ratio(x)) - x).abs() < 1e-12);
        }
    }

    #[test]
    fn spanish_labels_cover_the_scale() {
        assert_eq!(saaty_label_es(1), "Igual");
        assert_eq!(saaty_label_es(5), "Mucho mejor");
        assert_eq!(saaty_label_es(9), "Extremadamente mejor");
        assert_eq!(saaty_label_es(0), "Igual");
        assert_eq!(saaty_label_es(12), "Extremadamente mejor");
    }

    #[test]
    fn labels_and_num_var_for_small_sizes() {
        for n in 0..5 {
            let q = QuizPairMatrix::new(&labels(n), None);
            assert_eq!(q.num_var(), n);
            assert_eq!(q.labels(), labels(n));
            assert_eq!(q.scores().len(), n * n.saturating_sub(1) / 2);
        }
    }

    #[test]
    fn apply_signed_sets_scores_in_quiz_order() {
        let mut q = QuizPairMatrix::new(&labels(3), None);
        q.apply_signed(&[3.0, -5.0, 1.0]).unwrap();
        assert_eq!(q.to_signed(), vec![3.0, -5.0, 1.0]);
        assert!(q.apply_signed(&[1.0]).is_err());
    }
}
