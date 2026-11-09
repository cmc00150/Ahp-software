//! Interactive pairwise-comparison questionnaire over a line-based input.
//!
//! Prompts, the scale legend and answer echoes go to the given writer (stderr in the binary),
//! so stdout only carries the results and answers can be piped through stdin.

use std::fmt;
use std::io::{self, BufRead, Write};

use ahp::{AhpScore, QuizPairMatrix, Value, saaty_label_es};

/// Parses one answer line into a score.
///
/// Accepted: an integer in `1..=9` (left option dominates) or `-9..=-1` (right option
/// dominates); `1`, `-1` and an empty line mean "equal". Surrounding whitespace is ignored.
pub fn parse_answer(line: &str) -> Result<AhpScore, String> {
    let text = line.trim();
    if text.is_empty() {
        return Ok(AhpScore::Equal);
    }
    let invalid = || {
        format!(
            "Respuesta no válida: «{text}». Escribe un entero entre 1 y 9 o entre -9 y -1 \
             (línea vacía = igual)."
        )
    };
    let value: i32 = text.parse().map_err(|_| invalid())?;
    if !(1..=9).contains(&value.abs()) {
        return Err(invalid());
    }
    Ok(AhpScore::from_signed(f64::from(value)))
}

/// Human-readable meaning of a score for the pair `(left, right)`.
pub fn describe(score: AhpScore, left: &str, right: &str) -> String {
    match score {
        AhpScore::Equal => format!("{left} es «{}» que {right}", saaty_label_es(1)),
        AhpScore::LeftDominates(s) => {
            format!("{left} es «{}» que {right}", saaty_label_es(s as u8))
        }
        AhpScore::RightDominates(s) => {
            format!("{right} es «{}» que {left}", saaty_label_es(s as u8))
        }
    }
}

/// Formats a reference value compactly: integers without decimals, otherwise up to three
/// decimals with trailing zeros removed.
pub fn format_value(value: &Value) -> String {
    let x = value.data;
    let number = if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.0}")
    } else {
        let s = format!("{x:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    if value.unit.trim().is_empty() {
        number
    } else {
        format!("{number} {}", value.unit)
    }
}

fn side(label: &str, value: Option<&Value>) -> String {
    match value {
        Some(v) => format!("{label} ({})", format_value(v)),
        None => label.to_owned(),
    }
}

/// The legend of the answer scale, printed once before the first question.
pub fn legend() -> String {
    let mut out = String::from(
        "Escala de comparación (Saaty):\n  \
         Escribe un entero de 1 a 9 si la opción de la IZQUIERDA es mejor,\n  \
         o de -1 a -9 si lo es la de la DERECHA. 1, -1 o línea vacía: son iguales.\n",
    );
    for level in 1..=9u8 {
        out.push_str(&format!("    {level}  {}\n", saaty_label_es(level)));
    }
    out
}

#[derive(Debug)]
pub enum AskError {
    /// The input ended before every question was answered.
    Eof {
        header: String,
    },
    Io(io::Error),
}

impl fmt::Display for AskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AskError::Eof { header } => write!(
                f,
                "La entrada terminó antes de completar el cuestionario «{header}»."
            ),
            AskError::Io(e) => write!(f, "Error de entrada/salida: {e}"),
        }
    }
}

impl From<io::Error> for AskError {
    fn from(e: io::Error) -> Self {
        AskError::Io(e)
    }
}

/// Asks questionnaires on `input`, writing prompts to `out`.
pub struct Prompter<R, W> {
    input: R,
    out: W,
    legend_shown: bool,
}

impl<R: BufRead, W: Write> Prompter<R, W> {
    pub fn new(input: R, out: W) -> Self {
        Self {
            input,
            out,
            legend_shown: false,
        }
    }

    /// Asks every item of `quiz` under `header` and returns the signed Saaty answers in quiz
    /// order. Invalid answers are reported and asked again.
    pub fn ask(&mut self, header: &str, quiz: &QuizPairMatrix) -> Result<Vec<f64>, AskError> {
        let items = quiz.scores();
        if items.is_empty() {
            return Ok(Vec::new());
        }
        if !self.legend_shown {
            write!(self.out, "{}", legend())?;
            self.legend_shown = true;
        }
        writeln!(self.out, "\n== {header} ==")?;

        let total = items.len();
        let mut answers = Vec::with_capacity(total);
        let mut line = String::new();
        for (k, item) in items.iter().enumerate() {
            writeln!(
                self.out,
                "[{}/{total}] {}  vs  {}",
                k + 1,
                side(&item.label1, item.ref_value1.as_ref()),
                side(&item.label2, item.ref_value2.as_ref()),
            )?;
            loop {
                write!(self.out, "Respuesta: ")?;
                self.out.flush()?;
                line.clear();
                if self.input.read_line(&mut line)? == 0 {
                    writeln!(self.out)?;
                    return Err(AskError::Eof {
                        header: header.to_owned(),
                    });
                }
                match parse_answer(&line) {
                    Ok(score) => {
                        writeln!(
                            self.out,
                            "→ {}",
                            describe(score, &item.label1, &item.label2)
                        )?;
                        answers.push(f64::from(score.to_signed()));
                        break;
                    }
                    Err(msg) => writeln!(self.out, "{msg}")?,
                }
            }
        }
        Ok(answers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahp::{RowWithUnits, SaatyScore};
    use std::io::Cursor;

    #[test]
    fn parses_valid_answers() {
        assert_eq!(parse_answer(""), Ok(AhpScore::Equal));
        assert_eq!(parse_answer("  \n"), Ok(AhpScore::Equal));
        assert_eq!(parse_answer("1"), Ok(AhpScore::Equal));
        assert_eq!(parse_answer("-1"), Ok(AhpScore::Equal));
        assert_eq!(
            parse_answer(" 5\n"),
            Ok(AhpScore::LeftDominates(SaatyScore::StronglyBetter))
        );
        assert_eq!(
            parse_answer("+9"),
            Ok(AhpScore::LeftDominates(SaatyScore::ExtremelyBetter))
        );
        assert_eq!(
            parse_answer("-3"),
            Ok(AhpScore::RightDominates(SaatyScore::SlightlyBetter))
        );
    }

    #[test]
    fn rejects_invalid_answers() {
        for bad in ["0", "10", "-10", "2.5", "abc", "3 4"] {
            assert!(parse_answer(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn describes_direction_and_label() {
        let left = AhpScore::LeftDominates(SaatyScore::StronglyBetter);
        let right = AhpScore::RightDominates(SaatyScore::SlightlyBetter);
        assert_eq!(describe(left, "A", "B"), "A es «Mucho mejor» que B");
        assert_eq!(describe(right, "A", "B"), "B es «Algo mejor» que A");
        assert_eq!(describe(AhpScore::Equal, "A", "B"), "A es «Igual» que B");
    }

    #[test]
    fn formats_reference_values() {
        let v = |data, unit: &str| Value {
            data,
            unit: unit.to_owned(),
        };
        assert_eq!(format_value(&v(20000.0, "€")), "20000 €");
        assert_eq!(format_value(&v(5.6, "L/100km")), "5.6 L/100km");
        assert_eq!(format_value(&v(41.2, "")), "41.2");
    }

    fn quiz() -> QuizPairMatrix {
        let labels = ["A", "B", "C"].map(String::from);
        let row: RowWithUnits = ("€".to_owned(), vec![10.0, 20.0, 30.0]);
        QuizPairMatrix::new(&labels, Some(&row))
    }

    #[test]
    fn asks_every_pair_and_reasks_invalid_input() {
        let input = Cursor::new("x\n5\n\n-3\n");
        let mut out = Vec::new();
        let answers = Prompter::new(input, &mut out)
            .ask("Prueba", &quiz())
            .unwrap();
        assert_eq!(answers, vec![5.0, 1.0, -3.0]);

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Escala de comparación"));
        assert!(text.contains("== Prueba =="));
        assert!(text.contains("[1/3] A (10 €)  vs  B (20 €)"));
        assert!(text.contains("Respuesta no válida: «x»"));
        assert!(text.contains("→ A es «Mucho mejor» que B"));
        assert!(text.contains("→ C es «Algo mejor» que B"));
    }

    #[test]
    fn prints_legend_only_once() {
        let mut out = Vec::new();
        let mut p = Prompter::new(Cursor::new("1\n1\n1\n1\n1\n1\n"), &mut out);
        p.ask("Uno", &quiz()).unwrap();
        p.ask("Dos", &quiz()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("Escala de comparación").count(), 1);
    }

    #[test]
    fn fails_on_premature_eof() {
        let mut out = Vec::new();
        let err = Prompter::new(Cursor::new("3\n"), &mut out)
            .ask("Corto", &quiz())
            .unwrap_err();
        assert!(matches!(err, AskError::Eof { .. }));
        assert!(err.to_string().contains("«Corto»"));
    }
}
