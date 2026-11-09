//! Pure conversions shared by the screens: the 17-point Saaty scale, text parsing, pairwise
//! matrix indexing and number formatting. Nothing here depends on Slint.

use ahp::{ratio_to_signed, saaty_label_es};

/// Number of dots on the Saaty scale (9 levels to each side, sharing the "equal" dot).
pub const SCALE_POSITIONS: i32 = 17;
/// Dot that means "equal" (signed value `1`).
pub const EQUAL_POSITION: i32 = 8;

/// Signed Saaty value of a dot: `p < 8` => the LEFT item dominates (`p = 0` is `9`),
/// `p = 8` => equal (`1`), `p > 8` => the RIGHT item dominates (`p = 16` is `-9`).
/// Positions outside `0..=16` are clamped.
pub fn position_to_signed(position: i32) -> f64 {
    let p = position.clamp(0, SCALE_POSITIONS - 1);
    match p.cmp(&EQUAL_POSITION) {
        std::cmp::Ordering::Less => f64::from(9 - p),
        std::cmp::Ordering::Equal => 1.0,
        std::cmp::Ordering::Greater => -f64::from(p - 7),
    }
}

/// Nearest dot for a signed value (non-integer values coming from matrix edits are rounded,
/// `|x|` is clamped to `1..=9`; non-finite values map to "equal").
pub fn signed_to_position(x: f64) -> i32 {
    if !x.is_finite() {
        return EQUAL_POSITION;
    }
    let level = x.abs().round().clamp(1.0, 9.0) as i32;
    if level == 1 {
        EQUAL_POSITION
    } else if x > 0.0 {
        9 - level
    } else {
        7 + level
    }
}

/// Spanish label of a dot ("Igual", "Algo mejor", ...).
pub fn position_label(position: i32) -> &'static str {
    saaty_label_es(position_to_signed(position).abs() as u8)
}

/// Direction of the arrow drawn behind the label: `1` points RIGHT (the left item dominates),
/// `-1` points LEFT (the right item dominates) and `0` means no arrow (equal).
pub fn arrow_direction(position: i32) -> i32 {
    match position.cmp(&EQUAL_POSITION) {
        std::cmp::Ordering::Less => 1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => -1,
    }
}

/// Parses a data value. Accepts a comma as decimal separator.
pub fn parse_number(text: &str) -> Result<f64, String> {
    let clean = text.trim().replace(',', ".");
    if clean.is_empty() {
        return Err("el valor está vacío".into());
    }
    match clean.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(format!("\"{}\" no es un número válido", text.trim())),
    }
}

/// Parses a pairwise matrix cell: a positive decimal (`0.25`, `0,25`) or fraction (`1/3`).
pub fn parse_ratio(text: &str) -> Result<f64, String> {
    let shown = text.trim();
    let invalid = || {
        format!("\"{shown}\" no es válido: escribe un número positivo (por ejemplo 3, 0.25 o 1/3)")
    };
    let value = match shown.split_once('/') {
        Some((num, den)) => {
            let num = parse_number(num).map_err(|_| invalid())?;
            let den = parse_number(den).map_err(|_| invalid())?;
            if den == 0.0 {
                return Err(invalid());
            }
            num / den
        }
        None => parse_number(shown).map_err(|_| invalid())?,
    };
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(invalid())
    }
}

/// Index of the pair `(i, j)` (`i < j`) in quiz order `(0,1), (0,2), ..., (1,2), ...` of an
/// `n x n` pairwise matrix.
pub fn pair_index(n: usize, i: usize, j: usize) -> usize {
    debug_assert!(i < j && j < n);
    i * n - i * (i + 1) / 2 + (j - i - 1)
}

/// Stores the matrix cell `a_ij = ratio` (and therefore `a_ji = 1/ratio`) as a signed value
/// in the quiz-ordered `values` of an `n x n` matrix.
pub fn set_pair(
    values: &mut [f64],
    n: usize,
    i: usize,
    j: usize,
    ratio: f64,
) -> Result<(), String> {
    if i == j || i >= n || j >= n {
        return Err("la diagonal de la matriz siempre vale 1".into());
    }
    if !(ratio.is_finite() && ratio > 0.0) {
        return Err("el valor debe ser un número positivo".into());
    }
    let (row, col, r) = if i < j {
        (i, j, ratio)
    } else {
        (j, i, 1.0 / ratio)
    };
    let slot = values
        .get_mut(pair_index(n, row, col))
        .ok_or("la matriz no corresponde a las evaluaciones")?;
    *slot = ratio_to_signed(r);
    Ok(())
}

/// Visual scale of the ranking item at `rank` (0 = first): 1.5, 1.3, 1.1, ... never below 0.7.
pub fn rank_scale(rank: usize) -> f32 {
    (1.5 - 0.2 * rank as f32).max(0.7)
}

/// Data value without useless decimals (`18500`, `5.6`).
pub fn fmt_value(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        trim_decimals(format!("{x:.6}"))
    }
}

/// Matrix cell: integers as such, reciprocals of integers as `1/k`, otherwise 3 decimals.
pub fn fmt_ratio(r: f64) -> String {
    const EPS: f64 = 1e-9;
    if (r - r.round()).abs() < EPS {
        return format!("{}", r.round() as i64);
    }
    if r > 0.0 && r < 1.0 {
        let inv = 1.0 / r;
        if (inv - inv.round()).abs() < 1e-6 {
            return format!("1/{}", inv.round() as i64);
        }
    }
    trim_decimals(format!("{r:.3}"))
}

/// Weight or priority with 4 decimals.
pub fn fmt_weight(w: f64) -> String {
    format!("{w:.4}")
}

fn trim_decimals(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_position_round_trips_through_the_signed_value() {
        let expected = [
            9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0, -2.0, -3.0, -4.0, -5.0, -6.0, -7.0, -8.0,
            -9.0,
        ];
        for p in 0..SCALE_POSITIONS {
            let x = position_to_signed(p);
            assert_eq!(x, expected[p as usize], "position {p}");
            assert_eq!(signed_to_position(x), p, "round trip of {x}");
        }
    }

    #[test]
    fn signed_values_map_to_the_nearest_dot() {
        assert_eq!(signed_to_position(-1.0), EQUAL_POSITION);
        assert_eq!(signed_to_position(1.4), EQUAL_POSITION);
        assert_eq!(signed_to_position(2.6), 6);
        assert_eq!(signed_to_position(-3.2), 10);
        assert_eq!(signed_to_position(25.0), 0);
        assert_eq!(signed_to_position(-25.0), 16);
        assert_eq!(signed_to_position(f64::NAN), EQUAL_POSITION);
        assert_eq!(position_to_signed(-3), 9.0);
        assert_eq!(position_to_signed(40), -9.0);
    }

    #[test]
    fn labels_follow_the_intensity() {
        assert_eq!(position_label(8), "Igual");
        assert_eq!(position_label(6), "Algo mejor");
        assert_eq!(position_label(10), "Algo mejor");
        assert_eq!(position_label(4), "Mucho mejor");
        assert_eq!(position_label(16), "Extremadamente mejor");
    }

    #[test]
    fn arrow_points_away_from_the_dominant_side() {
        for p in 0..EQUAL_POSITION {
            assert_eq!(
                arrow_direction(p),
                1,
                "left dominates at {p}: arrow points right"
            );
        }
        assert_eq!(arrow_direction(EQUAL_POSITION), 0);
        for p in EQUAL_POSITION + 1..SCALE_POSITIONS {
            assert_eq!(
                arrow_direction(p),
                -1,
                "right dominates at {p}: arrow points left"
            );
        }
    }

    #[test]
    fn parses_matrix_cells() {
        assert_eq!(parse_ratio("3"), Ok(3.0));
        assert_eq!(parse_ratio(" 0.25 "), Ok(0.25));
        assert_eq!(parse_ratio("0,5"), Ok(0.5));
        assert!((parse_ratio("1/3").unwrap() - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(parse_ratio("2 / 4"), Ok(0.5));
        for bad in [
            "", "abc", "0", "-2", "1/0", "1/", "/3", "1/2/3", "inf", "NaN", "-1/3",
        ] {
            assert!(parse_ratio(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn parses_data_values() {
        assert_eq!(parse_number("18500"), Ok(18500.0));
        assert_eq!(parse_number("5,6"), Ok(5.6));
        assert_eq!(parse_number("-3"), Ok(-3.0));
        assert!(parse_number("").is_err());
        assert!(parse_number("x").is_err());
        assert!(parse_number("inf").is_err());
    }

    #[test]
    fn pair_indices_follow_quiz_order() {
        let n = 4;
        let order = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
        for (k, (i, j)) in order.into_iter().enumerate() {
            assert_eq!(pair_index(n, i, j), k);
        }
    }

    #[test]
    fn editing_a_cell_stores_the_signed_value_of_the_pair() {
        let mut v = vec![1.0; 6];
        set_pair(&mut v, 4, 0, 2, 5.0).unwrap();
        assert_eq!(v[1], 5.0);
        // a_31 = 1/3 => a_13 = 3 (left dominates).
        set_pair(&mut v, 4, 3, 1, 1.0 / 3.0).unwrap();
        assert!((v[4] - 3.0).abs() < 1e-12);
        // a_12 = 0.25 => right dominates with 4.
        set_pair(&mut v, 4, 1, 2, 0.25).unwrap();
        assert_eq!(v[3], -4.0);
        // a_32 = 4 => a_23 = 1/4.
        set_pair(&mut v, 4, 3, 2, 4.0).unwrap();
        assert_eq!(v[5], -4.0);
        assert!(set_pair(&mut v, 4, 1, 1, 2.0).is_err());
        assert!(set_pair(&mut v, 4, 0, 1, 0.0).is_err());
    }

    #[test]
    fn rank_scale_shrinks_and_is_clamped() {
        let scales: Vec<f32> = (0..8).map(rank_scale).collect();
        let expected = [1.5, 1.3, 1.1, 0.9, 0.7, 0.7, 0.7, 0.7];
        for (s, e) in scales.iter().zip(expected) {
            assert!((s - e).abs() < 1e-6, "{scales:?}");
        }
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(fmt_value(18500.0), "18500");
        assert_eq!(fmt_value(5.6), "5.6");
        assert_eq!(fmt_ratio(3.0), "3");
        assert_eq!(fmt_ratio(1.0 / 7.0), "1/7");
        assert_eq!(fmt_ratio(0.4), "0.4");
        assert_eq!(fmt_ratio(2.5), "2.5");
        assert_eq!(fmt_weight(0.123456), "0.1235");
    }
}
