pub mod ahp_classic;
pub mod ahp_shortII;
pub mod share;

#[cfg(test)]
mod tests {
    use crate::share::{
        pair_matrix::PairMatrix,
        pesos::WeightMethod,
        quiz::{AhpScore, QuizPairMatrix, SaatyScore},
    };

    fn redondear(x: f64, decimales: u32) -> f64 {
        let factor = 10f64.powi(decimales as i32);
        (x * factor).round() / factor
    }

    fn matriz_ejemplo() -> PairMatrix {
        let criteria: Vec<String> = ["c1", "c2", "c3"].iter().map(|s| s.to_string()).collect();
        let mut q = QuizPairMatrix::new(&criteria, None);
        q.set_score(0, AhpScore::LeftDominates(SaatyScore::StronglyBetterPlus));
        q.set_score(1, AhpScore::LeftDominates(SaatyScore::SlightlyBetter));
        q.set_score(2, AhpScore::RightDominates(SaatyScore::EqualPlus));
        PairMatrix::from_quiz(q)
    }

    #[test]
    fn matriz_creada_correctamente() {
        let m = matriz_ejemplo();
        let esperada = [[1.0, 6.0, 3.0], [0.2, 1.0, 0.5], [0.3, 2.0, 1.0]];

        let mut errores = Vec::new();
        for i in 0..3 {
            for j in 0..3 {
                let obtenido = redondear(m.get(i, j).expect("índice dentro de rango"), 1);
                if obtenido != esperada[i][j] {
                    errores.push(format!(
                        "({i},{j}): esperado {}, obtenido {obtenido}",
                        esperada[i][j]
                    ));
                }
            }
        }

        assert!(
            errores.is_empty(),
            "celdas incorrectas:\n{}",
            errores.join("\n")
        );
    }

    #[test]
    fn pesos() {
        let v = WeightMethod::EigenValues.calc_weights(&matriz_ejemplo());
        assert_eq!(
            v.iter().map(|val| redondear(*val, 3)).collect::<Vec<f64>>(),
            vec![0.667, 0.111, 0.222],
            "Autovector"
        );
    }

    #[test]
    fn lambda_max() {
        let m = matriz_ejemplo();
        let v = WeightMethod::EigenValues.calc_weights(&m);
        let l = WeightMethod::EigenValues
            .calc_lambda_max(&v, &m)
            .expect("calc_lambda_max devolvió None");
        assert_eq!(redondear(l, 3), 3.000, "lambda max");
    }

    #[test]
    fn ci() {
        let m = matriz_ejemplo();
        let v = WeightMethod::EigenValues.calc_weights(&m);
        let (ci, _cr) = WeightMethod::EigenValues
            .calc_cicr(&v, &m)
            .expect("calc_cicr devolvió None");
        assert_eq!(redondear(ci, 3), 0.000, "CI");
    }

    #[test]
    fn cr() {
        let m = matriz_ejemplo();
        let v = WeightMethod::EigenValues.calc_weights(&m);
        let (_ci, cr) = WeightMethod::EigenValues
            .calc_cicr(&v, &m)
            .expect("calc_cicr devolvió None");
        assert_eq!(redondear(cr, 3), 0.000, "CR");
    }
}
