use super::*;

fn example(name: &str) -> Project {
    let path = format!("{}/../examples/{name}", env!("CARGO_MANIFEST_DIR"));
    Project::load(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn assert_close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "expected {b}, got {a}");
}

/// Range evaluations where a lower value is preferred (as in the London example).
fn lower_is_better(project: &Project) -> Vec<Vec<f64>> {
    let (_, quizzes) = short_quizzes(project).unwrap();
    quizzes
        .iter()
        .enumerate()
        .map(|(k, (_, q))| {
            let range = project.effective_range(k);
            let d = |v: f64| 9.0 - 8.0 * v / range;
            q.scores()
                .iter()
                .map(|it| {
                    let r = d(it.ref_value1.as_ref().unwrap().data)
                        / d(it.ref_value2.as_ref().unwrap().data);
                    ratio_to_signed_rounded(r)
                })
                .collect()
        })
        .collect()
}

fn ratio_to_signed_rounded(r: f64) -> f64 {
    let s = if r >= 1.0 {
        r.round()
    } else {
        -(1.0 / r).round()
    };
    if s.abs() <= 1.0 { 1.0 } else { s }
}

#[test]
fn every_example_round_trips_through_toml() {
    for name in [
        "coches.ahp",
        "coches_sin_evaluar.ahp",
        "londres.ahp",
        "londres_parcial.ahp",
    ] {
        let p = example(name);
        let text = p.to_toml_string().unwrap();
        assert_eq!(Project::from_toml_str(&text).unwrap(), p, "{name}");
    }
}

#[test]
fn saved_files_skip_empty_fields() {
    let text = example("coches_sin_evaluar.ahp").to_toml_string().unwrap();
    for absent in ["range", "profiles", "categories", "evaluations"] {
        assert!(!text.contains(absent), "unexpected `{absent}` in:\n{text}");
    }
}

#[test]
fn parse_errors_are_readable() {
    let err = Project::from_toml_str("alternatives = [\"a\"]\nfoo = 1\n").unwrap_err();
    assert!(matches!(err, ProjectError::Parse(_)));
    assert!(err.to_string().contains("foo"), "{err}");
    let err = Project::from_toml_str("method = \"other\"").unwrap_err();
    assert!(matches!(err, ProjectError::Parse(_)));
}

#[test]
fn effective_range_defaults_to_max_value() {
    let mut p = example("coches_sin_evaluar.ahp");
    assert_eq!(p.effective_range(0), 31000.0);
    p.criteria[0].range = Some(50000.0);
    assert_eq!(p.effective_range(0), 50000.0);
    p.criteria[1].values.clear();
    assert_eq!(p.effective_range(1), 0.0);
    assert_eq!(p.effective_range(99), 0.0);
}

#[test]
fn solves_the_classic_example() {
    let p = example("coches.ahp");
    let (ev, missing) = Evaluations::from_project(&p, Method::Classic).unwrap();
    assert!(missing.is_complete());

    let out = solve_classic(&p, &ev).unwrap();
    assert_eq!(out.ranking.len(), 4);
    assert!(
        out.ranking.windows(2).all(|w| w[0].1 >= w[1].1),
        "descending"
    );
    assert_close(out.ranking.iter().map(|r| r.1).sum(), 1.0, 1e-9);
    assert_close(out.criteria.report.weights.iter().sum(), 1.0, 1e-9);
    assert_eq!(out.criteria.labels, p.criteria_names());
    assert_eq!(out.alternatives.len(), 4);
    for (_, m) in &out.alternatives {
        assert_eq!(m.labels, p.alternatives);
        assert_eq!(m.cells.len(), 4);
        assert_close(m.report.weights.iter().sum(), 1.0, 1e-9);
    }
    // Precio-Consumo = 3 => a_01 = 3, a_10 = 1/3
    assert_close(out.criteria.cells[0][1], 3.0, 1e-12);
    assert_close(out.criteria.cells[1][0], 1.0 / 3.0, 1e-12);
}

#[test]
fn solve_uses_the_exact_signed_values() {
    let p = example("coches.ahp");
    let (mut ev, _) = Evaluations::from_project(&p, Method::Classic).unwrap();
    ev.criteria[0] = 2.5;
    ev.per_criterion[0][0] = -3.5;
    let out = solve_classic(&p, &ev).unwrap();
    assert_close(out.criteria.cells[0][1], 2.5, 1e-12);
    assert_close(out.alternatives[0].1.cells[0][1], 1.0 / 3.5, 1e-12);
    assert_close(out.alternatives[0].1.cells[1][0], 3.5, 1e-12);
}

#[test]
fn solves_the_short_example_with_a_mix_of_categories() {
    let p = example("londres.ahp");
    let (ev, missing) = Evaluations::from_project(&p, Method::ShortII).unwrap();
    assert!(missing.is_complete());

    let out = solve_short(&p, &ev).unwrap();
    let names = &p.categories.as_ref().unwrap().names;
    assert_eq!(out.classification.len(), p.alternatives.len());
    assert!(out.classification.iter().all(|(_, _, c)| names.contains(c)));
    let distinct: HashSet<&str> = out
        .classification
        .iter()
        .map(|(_, _, c)| c.as_str())
        .collect();
    assert!(distinct.len() >= 2, "{:?}", out.classification);

    assert_eq!(out.profiles.len(), 3);
    assert_eq!(out.alternatives.len(), 6);
    assert!(
        out.alternatives
            .iter()
            .all(|(_, local, _)| local.len() == 5)
    );
    for (_, points) in &out.local_priorities {
        assert_eq!(points.len(), 9);
        assert!(points.windows(2).all(|w| w[0].original <= w[1].original));
        assert_eq!(points.iter().filter(|p| p.profile.is_some()).count(), 3);
        assert_eq!(points[0].label, "RP1");
    }
    // The point weights are the eigenvector of the range matrix
    let (_, points) = &out.local_priorities[0];
    let (_, matrix) = &out.ranges[0];
    for point in points {
        let idx = matrix
            .labels
            .iter()
            .position(|l| *l == point.label)
            .unwrap();
        assert_close(point.weight, matrix.report.weights[idx], 1e-12);
    }
}

#[test]
fn quiz_sizes_match_the_library_quizzes() {
    let p = example("londres.ahp");
    let (c, r) = short_quizzes(&p).unwrap();
    let (cs, rs) = p.quiz_sizes(Method::ShortII);
    assert_eq!(c.scores().len(), cs);
    assert_eq!(
        r.iter().map(|(_, q)| q.scores().len()).collect::<Vec<_>>(),
        rs
    );

    let p = example("coches.ahp");
    let (c, a) = classic_quizzes(&p).unwrap();
    let (cs, as_) = p.quiz_sizes(Method::Classic);
    assert_eq!(c.scores().len(), cs);
    assert_eq!(
        a.iter().map(|(_, q)| q.scores().len()).collect::<Vec<_>>(),
        as_
    );
}

#[test]
fn detects_missing_parts_of_partial_files() {
    let (ev, missing) =
        Evaluations::from_project(&example("coches_sin_evaluar.ahp"), Method::Classic).unwrap();
    assert!(missing.is_empty_file());
    assert_eq!(
        ev,
        Evaluations::defaults(&example("coches.ahp"), Method::Classic)
    );

    let p = example("londres_parcial.ahp");
    let (ev, missing) = Evaluations::from_project(&p, Method::ShortII).unwrap();
    assert!(!missing.criteria);
    assert_eq!(missing.per_criterion, vec![true; 5]);
    assert_eq!(&ev.criteria, p.evaluations.criteria.as_ref().unwrap());
    assert!(ev.per_criterion.iter().flatten().all(|&x| x == 1.0));
}

#[test]
fn stored_evaluations_reload_complete() {
    let mut p = example("londres_parcial.ahp");
    let (mut ev, _) = Evaluations::from_project(&p, Method::ShortII).unwrap();
    ev.per_criterion = lower_is_better(&p);
    ev.store_into(&mut p, Method::ShortII);

    let reloaded = Project::from_toml_str(&p.to_toml_string().unwrap()).unwrap();
    let (again, missing) = Evaluations::from_project(&reloaded, Method::ShortII).unwrap();
    assert!(missing.is_complete());
    assert_eq!(again, ev);
    // Keyed by criterion name, in criterion order
    let keys: Vec<&str> = reloaded
        .evaluations
        .ranges
        .0
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(keys, p.criteria_names());
    // Same results as the shipped complete example
    assert_eq!(solve_short(&reloaded, &again).unwrap().classification, {
        let full = example("londres.ahp");
        let (ev, _) = Evaluations::from_project(&full, Method::ShortII).unwrap();
        solve_short(&full, &ev).unwrap().classification
    });
}

#[test]
fn limiting_profiles_use_the_last_category_as_catch_all() {
    let text = r#"
        alternatives = ["bueno", "medio", "malo"]

        [[criteria]]
        name = "c1"
        values = [10.0, 50.0, 95.0]
        range = 100.0
        profiles = [30.0, 70.0]

        [[criteria]]
        name = "c2"
        values = [5.0, 45.0, 90.0]
        range = 100.0
        profiles = [30.0, 70.0]

        [categories]
        profile_type = "limiting"
        names = ["A", "B", "C"]
    "#;
    let p = Project::from_toml_str(text).unwrap();
    let (mut ev, _) = Evaluations::from_project(&p, Method::ShortII).unwrap();
    ev.per_criterion = lower_is_better(&p);
    let out = solve_short(&p, &ev).unwrap();

    assert_eq!(out.profiles.len(), 2, "the last category has no profile");
    let class = |alt: &str| {
        out.classification
            .iter()
            .find(|(a, _, _)| a == alt)
            .map(|(_, _, c)| c.as_str())
            .unwrap()
    };
    assert!(
        out.classification
            .iter()
            .all(|(_, _, c)| ["A", "B", "C"].contains(&c.as_str()))
    );
    // Lower values are preferred, so profile A (30) outranks profile B (70). Categories are
    // bounded by ascending profile priority: below B's limit => B, up to A's limit => A,
    // and the catch-all C collects the alternative above every limit.
    assert_eq!(class("malo"), "B");
    assert_eq!(class("medio"), "A");
    assert_eq!(class("bueno"), "C");
}

#[test]
fn validation_rejects_inconsistent_projects() {
    let base = example("londres.ahp");
    let expect_invalid = |p: &Project, method: Method, needle: &str| {
        let err = p.validate(method).unwrap_err();
        assert!(matches!(err, ProjectError::Invalid(_)), "{err}");
        assert!(
            err.to_string().contains(needle),
            "`{needle}` not in `{err}`"
        );
    };

    let mut p = base.clone();
    p.criteria[0].values.pop();
    expect_invalid(&p, Method::ShortII, "valores");

    let mut p = base.clone();
    p.alternatives[1] = p.alternatives[0].clone();
    expect_invalid(&p, Method::ShortII, "repetida");

    let mut p = base.clone();
    p.criteria[2].profiles.pop();
    expect_invalid(&p, Method::ShortII, "perfiles");

    let mut p = base.clone();
    p.categories.as_mut().unwrap().profile_type = ProfileType::Limiting;
    expect_invalid(&p, Method::ShortII, "perfiles");

    let mut p = base.clone();
    p.categories = None;
    expect_invalid(&p, Method::ShortII, "categories");

    let mut p = base.clone();
    p.evaluations.ranges.0[0].1.pop();
    expect_invalid(&p, Method::ShortII, "debe tener");

    let mut p = base.clone();
    p.evaluations.ranges.0[0].0 = "No existe".into();
    expect_invalid(&p, Method::ShortII, "No existe");

    let mut p = base.clone();
    p.evaluations.criteria.as_mut().unwrap()[0] = 0.5;
    expect_invalid(&p, Method::ShortII, "|x| >= 1");

    let mut p = example("coches.ahp");
    p.criteria.truncate(1);
    p.evaluations = FileEvaluations::default();
    expect_invalid(&p, Method::Classic, "2 criterios");

    assert!(example("coches.ahp").validate_classic().is_ok());
    assert!(base.validate_short().is_ok());
}

#[test]
fn solve_rejects_evaluations_of_the_wrong_shape() {
    let p = example("coches.ahp");
    let mut ev = Evaluations::defaults(&p, Method::Classic);
    ev.per_criterion[1].pop();
    assert!(matches!(
        solve_classic(&p, &ev),
        Err(ProjectError::Invalid(_))
    ));
}
