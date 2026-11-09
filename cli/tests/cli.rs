//! End-to-end tests of the `ahp` binary on the example projects.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// Comparisons asked for `examples/coches_sin_evaluar.ahp`: 6 criteria + 4 x 6 alternatives.
const COCHES_QUESTIONS: usize = 30;

fn example(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn tmp_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_file(&path);
    path.to_string_lossy().into_owned()
}

/// Runs the binary with `args`, feeding `stdin` from a separate thread (so large prompt
/// output on stderr can never deadlock the test).
fn ahp(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ahp"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ahp");
    let mut pipe = child.stdin.take().unwrap();
    let input = stdin.to_owned();
    let writer = std::thread::spawn(move || {
        // The child may exit before reading everything; ignore broken pipes.
        let _ = pipe.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().expect("wait ahp");
    writer.join().unwrap();
    out
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn ones(n: usize) -> String {
    "1\n".repeat(n)
}

#[test]
fn classic_complete_verbose_prints_matrices_and_ranking() {
    let out = ahp(&["classic", &example("coches.ahp")], "");
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(stdout.contains("Matriz de comparación de criterios"));
    assert!(stdout.contains("Peso"));
    assert!(stdout.contains("λmax = "));
    assert!(stdout.contains("Alternativas según «Precio» (€)"));
    assert!(stdout.contains("Ranking"));
    for name in ["Ibiza", "Golf", "Corolla", "Clio"] {
        assert!(stdout.contains(name), "missing {name}");
    }
    assert!(
        !text(&out.stderr).contains("Respuesta"),
        "no questions expected"
    );
}

#[test]
fn classic_no_verbose_prints_only_descending_ranking() {
    let out = ahp(&["classic", &example("coches.ahp"), "--no-verbose"], "");
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(stdout.starts_with("Ranking"));
    assert!(!stdout.contains("Matriz"));
    assert!(!stdout.contains("Peso"));

    // Rows look like "│ 1 │ Clio │ 0.386 │": the scores must be descending.
    let scores: Vec<f64> = stdout
        .lines()
        .filter_map(|l| {
            let cells: Vec<_> = l.split('│').map(str::trim).collect();
            (cells.len() == 5 && cells[1].parse::<usize>().is_ok())
                .then(|| cells[3].parse().unwrap())
        })
        .collect();
    assert_eq!(scores.len(), 4, "{stdout}");
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
}

#[test]
fn short_complete_verbose_prints_every_section() {
    let out = ahp(&["shortII", &example("londres.ahp")], "");
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    for section in [
        "Matriz de comparación de criterios",
        "Puntos de referencia de «Crímenes»",
        "Prioridades locales de «Crímenes»",
        "Prioridades de las alternativas",
        "Prioridades de los perfiles",
        "Clasificación",
        "Prioridad global del perfil",
    ] {
        assert!(stdout.contains(section), "missing {section}");
    }
    for name in [
        "Barking",
        "Barnet",
        "Camden",
        "Hackney",
        "Richmond",
        "Westminster",
    ] {
        assert!(stdout.contains(name), "missing {name}");
    }
}

#[test]
fn short_no_verbose_prints_only_classification() {
    let out = ahp(&["short-ii", &example("londres.ahp"), "--no-verbose"], "");
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(stdout.starts_with("Clasificación"));
    assert!(!stdout.contains("Prioridades locales"));
}

#[test]
fn short_partial_answered_with_ones_succeeds() {
    let out = ahp(
        &["shortII", &example("londres_parcial.ahp"), "--no-verbose"],
        &ones(1000),
    );
    let stderr = text(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    // Criteria are in the file: only the range questionnaires are asked.
    assert!(!stderr.contains("Comparación de criterios"));
    assert!(stderr.contains("Criterio «Crímenes» (delitos/1000 hab.)"));
    assert!(stderr.contains("[1/36] RP6 (206 delitos/1000 hab.)  vs  RP5"));
    assert_eq!(stderr.matches("Escala de comparación").count(), 1);
    assert!(text(&out.stdout).contains("Clasificación"));
}

#[test]
fn short_partial_answered_with_a_mix_succeeds() {
    let out = ahp(
        &["shortII", &example("londres_parcial.ahp")],
        &"3\n-5\n\n2\n-1\n9\n".repeat(40),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("Prioridades locales"));
}

#[test]
fn classic_without_evaluations_and_short_stdin_fails() {
    let out = ahp(&["classic", &example("coches_sin_evaluar.ahp")], "3\n-5\n");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("La entrada terminó"));
    assert!(out.stdout.is_empty());
}

#[test]
fn answers_are_echoed_with_their_meaning() {
    let input = format!("5\n-3\n{}", ones(COCHES_QUESTIONS));
    let out = ahp(&["classic", &example("coches_sin_evaluar.ahp")], &input);
    let stderr = text(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("== Comparación de criterios =="));
    assert!(stderr.contains("[1/6] Precio  vs  Consumo"));
    assert!(stderr.contains("→ Precio es «Mucho mejor» que Consumo"));
    assert!(stderr.contains("→ Potencia es «Algo mejor» que Precio"));
    assert!(stderr.contains("Criterio «Precio» (€): comparación de alternativas"));
    assert!(stderr.contains("Ibiza (18500 €)  vs  Golf (29000 €)"));
}

#[test]
fn invalid_input_is_asked_again() {
    let input = format!("0\nabc\n10\n{}", ones(COCHES_QUESTIONS));
    let out = ahp(&["classic", &example("coches_sin_evaluar.ahp")], &input);
    let stderr = text(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(stderr.matches("Respuesta no válida").count(), 3);
    assert_eq!(stderr.matches("→ ").count(), COCHES_QUESTIONS);
}

#[test]
fn saved_project_runs_without_questions() {
    let saved = tmp_file("coches_guardado.ahp");
    let input = format!("5\n-3\n{}", ones(COCHES_QUESTIONS));
    let first = ahp(
        &[
            "classic",
            &example("coches_sin_evaluar.ahp"),
            "--save",
            &saved,
            "--no-verbose",
        ],
        &input,
    );
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(text(&first.stderr).contains("Proyecto guardado"));

    let second = ahp(&["classic", &saved, "--no-verbose"], "");
    assert!(second.status.success(), "{}", text(&second.stderr));
    assert!(!text(&second.stderr).contains("Respuesta"));
    assert_eq!(
        first.stdout, second.stdout,
        "same evaluations, same ranking"
    );
}

#[test]
fn saved_short_project_runs_without_questions() {
    let saved = tmp_file("londres_guardado.ahp");
    let first = ahp(
        &["shortII", &example("londres_parcial.ahp"), "--save", &saved],
        &ones(1000),
    );
    assert!(first.status.success(), "{}", text(&first.stderr));

    let second = ahp(&["shortII", &saved], "");
    assert!(second.status.success(), "{}", text(&second.stderr));
    assert!(!text(&second.stderr).contains("Respuesta"));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn missing_file_reports_error() {
    let out = ahp(&["classic", "no-existe.ahp"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("No se pudo cargar «no-existe.ahp»"));
}

#[test]
fn classic_project_is_rejected_by_short() {
    let out = ahp(&["shortII", &example("coches.ahp")], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).starts_with("Error: "));
}
