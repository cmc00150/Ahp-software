//! Minimal plain-text table formatter with box-drawing borders.
//!
//! Widths are measured in `char`s, which is enough for the precomposed accented letters
//! used in the project files (e.g. "Crímenes").

use std::fmt;

/// Horizontal alignment of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// A table with a header row and any number of body rows.
///
/// Rows shorter than the widest row are padded with empty cells.
#[derive(Debug, Clone, Default)]
pub struct Table {
    headers: Vec<String>,
    aligns: Vec<Align>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new<I, S>(headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            headers: headers.into_iter().map(Into::into).collect(),
            aligns: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// Sets the alignment of every column from `first` (inclusive) onwards.
    /// Columns without an explicit alignment are left-aligned.
    pub fn align_from(mut self, first: usize, align: Align) -> Self {
        let n = self.columns().max(first);
        self.aligns.resize(n, Align::Left);
        for a in &mut self.aligns[first..] {
            *a = align;
        }
        self
    }

    pub fn add_row<I, S>(&mut self, cells: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rows.push(cells.into_iter().map(Into::into).collect());
    }

    fn columns(&self) -> usize {
        self.rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(self.headers.len()))
            .max()
            .unwrap_or(0)
    }

    fn align(&self, col: usize) -> Align {
        // Columns beyond the configured ones inherit the last configured alignment.
        self.aligns
            .get(col)
            .or(self.aligns.last())
            .copied()
            .unwrap_or(Align::Left)
    }

    fn widths(&self) -> Vec<usize> {
        let mut widths = vec![0; self.columns()];
        for row in std::iter::once(&self.headers).chain(&self.rows) {
            for (w, cell) in widths.iter_mut().zip(row) {
                *w = (*w).max(cell.chars().count());
            }
        }
        widths
    }

    fn border(widths: &[usize], left: char, mid: char, right: char) -> String {
        let inner = widths
            .iter()
            .map(|&w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join(&mid.to_string());
        format!("{left}{inner}{right}\n")
    }

    fn line(&self, widths: &[usize], cells: &[String]) -> String {
        let mut out = String::from("│");
        for (col, &w) in widths.iter().enumerate() {
            let cell = cells.get(col).map(String::as_str).unwrap_or("");
            let pad = w - cell.chars().count();
            let padded = match self.align(col) {
                Align::Left => format!("{cell}{}", " ".repeat(pad)),
                Align::Right => format!("{}{cell}", " ".repeat(pad)),
            };
            out.push(' ');
            out.push_str(&padded);
            out.push_str(" │");
        }
        out.push('\n');
        out
    }

    /// Renders the table; every line (including the last one) ends with `\n`.
    pub fn render(&self) -> String {
        let widths = self.widths();
        if widths.is_empty() {
            return String::new();
        }
        let mut out = Self::border(&widths, '┌', '┬', '┐');
        out.push_str(&self.line(&widths, &self.headers));
        out.push_str(&Self::border(&widths, '├', '┼', '┤'));
        for row in &self.rows {
            out.push_str(&self.line(&widths, row));
        }
        out.push_str(&Self::border(&widths, '└', '┴', '┘'));
        out
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Formats a number with exactly three decimals (the CLI-wide convention).
pub fn num(x: f64) -> String {
    let s = format!("{x:.3}");
    // Avoid printing "-0.000" for tiny negative values.
    if s.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        s.trim_start_matches('-').to_owned()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_borders_header_and_rows() {
        let mut t = Table::new(["Nombre", "Peso"]).align_from(1, Align::Right);
        t.add_row(["A", "0.500"]);
        t.add_row(["Largo", "1.000"]);
        let expected = "\
┌────────┬───────┐
│ Nombre │  Peso │
├────────┼───────┤
│ A      │ 0.500 │
│ Largo  │ 1.000 │
└────────┴───────┘
";
        assert_eq!(t.render(), expected);
    }

    #[test]
    fn measures_accented_text_in_chars() {
        let mut t = Table::new(["x"]);
        t.add_row(["Crímenes"]);
        let lines: Vec<_> = t.render().lines().map(|l| l.chars().count()).collect();
        assert!(lines.windows(2).all(|w| w[0] == w[1]), "{lines:?}");
    }

    #[test]
    fn pads_ragged_rows_and_extra_columns_inherit_alignment() {
        let mut t = Table::new(["a"]).align_from(0, Align::Right);
        t.add_row(["1", "22"]);
        t.add_row(["333"]);
        let out = t.render();
        assert!(out.contains("│   a │    │\n"), "{out}");
        assert!(out.contains("│   1 │ 22 │\n"), "{out}");
        assert!(out.contains("│ 333 │    │\n"), "{out}");
    }

    #[test]
    fn empty_table_renders_nothing() {
        assert_eq!(Table::new(Vec::<String>::new()).render(), "");
    }

    #[test]
    fn num_uses_three_decimals_without_negative_zero() {
        assert_eq!(num(0.12345), "0.123");
        assert_eq!(num(2.0), "2.000");
        assert_eq!(num(-0.0001), "0.000");
        assert_eq!(num(-1.5), "-1.500");
    }
}
