pub mod estructuras;
pub mod pesos;

#[cfg(test)]
#[cfg(test)]
mod tests {
    use crate::estructuras::{PairMatrix, QuizPairMatrix};
    use crate::pesos::WeightMethod;

    fn redondear(x: f64, decimales: u32) -> f64 {
        let factor = 10f64.powi(decimales as i32);
        (x * factor).round() / factor
    }

    fn matriz_ejemplo() -> PairMatrix {
        let mut q = QuizPairMatrix::new(vec!["c1".to_string(), "c2".to_string(), "c3".to_string()]);
        q.set_item(0, 6.0);
        q.set_item(1, 3.0);
        q.set_item(2, 1.0 / 2.0);
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
            .calc_CICR(&v, &m)
            .expect("calc_CICR devolvió None");
        assert_eq!(redondear(ci, 3), 0.000, "CI");
    }

    #[test]
    fn cr() {
        let m = matriz_ejemplo();
        let v = WeightMethod::EigenValues.calc_weights(&m);
        let (_ci, cr) = WeightMethod::EigenValues
            .calc_CICR(&v, &m)
            .expect("calc_CICR devolvió None");
        assert_eq!(redondear(cr, 3), 0.000, "CR");
    }
}
