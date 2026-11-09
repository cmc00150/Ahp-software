//! Plain-text rendering of solved projects.

use ahp::{ClassicOutcome, MatrixView, Project, WeightsReport, sortOutcome};

use crate::table::{Align, Table, num};

/// Placeholder for values that do not exist (e.g. a category without profile).
const NONE: &str = "—";

fn section(out: &mut String, title: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(title);
    out.push('\n');
    out.push_str(&"═".repeat(title.chars().count()));
    out.push('\n');
}

fn with_unit(name: &str, unit: &str) -> String {
    if unit.trim().is_empty() {
        format!("«{name}»")
    } else {
        format!("«{name}» ({unit})")
    }
}

fn unit_of<'a>(project: &'a Project, criterion: &str) -> &'a str {
    project
        .criteria
        .iter()
        .find(|c| c.name == criterion)
        .map(|c| c.unit.as_str())
        .unwrap_or("")
}

fn opt(x: Option<f64>) -> String {
    x.map(num).unwrap_or_else(|| NONE.to_owned())
}

fn consistency(report: &WeightsReport) -> String {
    let mut line = format!(
        "λmax = {}   CI = {}   CR = {}",
        opt(report.lambda_max),
        opt(report.ci),
        opt(report.cr)
    );
    if report.cr.is_some_and(|cr| cr > 0.1) {
        line.push_str("   (CR > 0.100: la matriz es inconsistente)");
    }
    line.push('\n');
    line
}

/// Pairwise matrix with a final "Peso" column, followed by its consistency line.
fn matrix(out: &mut String, title: &str, view: &MatrixView) {
    section(out, title);
    let headers = std::iter::once(String::new())
        .chain(view.labels.iter().cloned())
        .chain(std::iter::once("Peso".to_owned()));
    let mut table = Table::new(headers).align_from(1, Align::Right);
    for ((label, row), weight) in view
        .labels
        .iter()
        .zip(&view.cells)
        .zip(&view.report.weights)
    {
        let cells = std::iter::once(label.clone())
            .chain(row.iter().map(|&x| num(x)))
            .chain(std::iter::once(num(*weight)));
        table.add_row(cells);
    }
    out.push_str(&table.render());
    out.push_str(&consistency(&view.report));
}

fn ranking(out: &mut String, outcome: &ClassicOutcome) {
    section(out, "Ranking");
    let mut table = Table::new(["Posición", "Alternativa", "Prioridad"])
        .align_from(0, Align::Right)
        .align_from(1, Align::Left)
        .align_from(2, Align::Right);
    for (pos, (name, score)) in outcome.ranking.iter().enumerate() {
        table.add_row([(pos + 1).to_string(), name.clone(), num(*score)]);
    }
    out.push_str(&table.render());
}

/// Classic AHP report: matrices and weights when `verbose`, then the ranking.
pub fn classic(project: &Project, outcome: &ClassicOutcome, verbose: bool) -> String {
    let mut out = String::new();
    if verbose {
        matrix(
            &mut out,
            "Matriz de comparación de criterios",
            &outcome.criteria,
        );
        for (criterion, view) in &outcome.alternatives {
            let title = format!(
                "Alternativas según {}",
                with_unit(criterion, unit_of(project, criterion))
            );
            matrix(&mut out, &title, view);
        }
    }
    ranking(&mut out, outcome);
    out
}

/// Rows `(name, local priorities, global)` under the criteria columns plus "Global".
fn priorities(
    out: &mut String,
    title: &str,
    first: &str,
    criteria: &[String],
    rows: &[(String, Vec<f64>, f64)],
) {
    section(out, title);
    let headers = std::iter::once(first.to_owned())
        .chain(criteria.iter().cloned())
        .chain(std::iter::once("Global".to_owned()));
    let mut table = Table::new(headers).align_from(1, Align::Right);
    for (name, local, global) in rows {
        let cells = std::iter::once(name.clone())
            .chain(local.iter().map(|&x| num(x)))
            .chain(std::iter::once(num(*global)));
        table.add_row(cells);
    }
    out.push_str(&table.render());
}

fn classification(out: &mut String, outcome: &sortOutcome) {
    section(out, "Clasificación");
    let mut table = Table::new([
        "Alternativa",
        "Prioridad global",
        "Prioridad global del perfil",
        "Clase",
    ])
    .align_from(1, Align::Right)
    .align_from(3, Align::Left);
    for (alternative, global, category) in &outcome.classification {
        let profile = outcome
            .profiles
            .iter()
            .find(|(name, _, _)| name == category)
            .map(|(_, _, g)| num(*g))
            .unwrap_or_else(|| NONE.to_owned());
        table.add_row([alternative.clone(), num(*global), profile, category.clone()]);
    }
    out.push_str(&table.render());
}

/// AHPSort II report: matrices, local and global priorities when `verbose`, then the
/// classification.
pub fn sort(project: &Project, outcome: &sortOutcome, verbose: bool) -> String {
    let mut out = String::new();
    if verbose {
        matrix(
            &mut out,
            "Matriz de comparación de criterios",
            &outcome.criteria,
        );
        for (criterion, view) in &outcome.ranges {
            let title = format!(
                "Puntos de referencia de {}",
                with_unit(criterion, unit_of(project, criterion))
            );
            matrix(&mut out, &title, view);
        }
        for (criterion, points) in &outcome.local_priorities {
            section(
                &mut out,
                &format!(
                    "Prioridades locales de {}",
                    with_unit(criterion, unit_of(project, criterion))
                ),
            );
            let mut table = Table::new(["Punto", "Valor original", "Prioridad local"])
                .align_from(1, Align::Right);
            for p in points {
                table.add_row([p.label.clone(), num(p.original), num(p.weight)]);
            }
            out.push_str(&table.render());
        }
        let criteria: Vec<String> = outcome.criteria.labels.clone();
        priorities(
            &mut out,
            "Prioridades de las alternativas",
            "Alternativa",
            &criteria,
            &outcome.alternatives,
        );
        priorities(
            &mut out,
            "Prioridades de los perfiles",
            "Categoría",
            &criteria,
            &outcome.profiles,
        );
    }
    classification(&mut out, outcome);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(labels: &[&str], cr: Option<f64>) -> MatrixView {
        let n = labels.len();
        MatrixView {
            labels: labels.iter().map(|s| s.to_string()).collect(),
            cells: vec![vec![1.0; n]; n],
            report: WeightsReport {
                weights: vec![1.0 / n as f64; n],
                lambda_max: Some(n as f64),
                ci: Some(0.0),
                cr,
            },
        }
    }

    fn sort_outcome() -> sortOutcome {
        sortOutcome {
            criteria: view(&["C1"], None),
            ranges: Vec::new(),
            local_priorities: Vec::new(),
            alternatives: Vec::new(),
            profiles: vec![
                ("Alto".into(), vec![0.6], 0.6),
                ("Medio".into(), vec![0.3], 0.3),
            ],
            classification: vec![
                ("A".into(), 0.7, "Alto".into()),
                ("B".into(), 0.1, "Bajo".into()),
            ],
        }
    }

    #[test]
    fn classification_shows_profile_priority_or_dash() {
        let out = sort(&Project::default(), &sort_outcome(), false);
        assert!(out.starts_with("Clasificación\n"), "{out}");
        assert!(out.contains("│ A           │            0.700 │"), "{out}");
        let row_b = out.lines().find(|l| l.contains("│ B ")).unwrap();
        assert!(row_b.contains(" — │ Bajo"), "{row_b}");
        let row_a = out.lines().find(|l| l.contains("│ A ")).unwrap();
        assert!(row_a.contains("0.600"), "{row_a}");
    }

    #[test]
    fn matrix_has_weight_column_and_flags_inconsistency() {
        let mut out = String::new();
        matrix(&mut out, "Título", &view(&["x", "y"], Some(0.25)));
        assert!(out.starts_with("Título\n══════\n"), "{out}");
        assert!(out.contains("│   │     x │     y │  Peso │"), "{out}");
        assert!(out.contains("│ x │ 1.000 │ 1.000 │ 0.500 │"), "{out}");
        assert!(
            out.contains("CR = 0.250   (CR > 0.100: la matriz es inconsistente)"),
            "{out}"
        );
    }

    #[test]
    fn classic_ranking_keeps_given_order_with_positions() {
        let outcome = ClassicOutcome {
            criteria: view(&["C1"], Some(0.0)),
            alternatives: Vec::new(),
            ranking: vec![("Z".into(), 0.75), ("Y".into(), 0.25)],
        };
        let out = classic(&Project::default(), &outcome, false);
        assert!(out.starts_with("Ranking\n"), "{out}");
        let rows: Vec<_> = out.lines().filter(|l| l.contains("0.")).collect();
        assert_eq!(rows.len(), 2, "{out}");
        assert!(
            rows[0].contains("1 │ Z") && rows[0].contains("0.750"),
            "{out}"
        );
        assert!(
            rows[1].contains("2 │ Y") && rows[1].contains("0.250"),
            "{out}"
        );
    }
}
