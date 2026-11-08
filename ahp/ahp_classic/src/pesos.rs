use crate::estructuras::pair_matrix::PairMatrix;

const LIM_IT: u16 = 10_000;
const TOLERANCE: f64 = 1e-12;
const RI: [f64; 10] = [0.0, 0.0, 0.58, 0.9, 1.12, 1.24, 1.32, 1.41, 1.45, 1.49];

pub enum WeightMethod {
    Approximated,
    GeometricMean,
    EigenValues,
}

impl WeightMethod {
    pub(crate) fn calc_weights(&self, a: &PairMatrix) -> Vec<f64> {
        let n = a.get_size();

        match self {
            WeightMethod::Approximated => {
                // Se empieza con 1 y por n por la diagonal que es 1 siempre.
                let mut row_sums = vec![1.0; n];
                let mut total = n as f64;

                for i in 0..n {
                    for j in (i + 1)..n {
                        // Recorremos solo la parte superior
                        if let Some(v) = a.get(i, j) {
                            row_sums[i] += v;
                            row_sums[j] += 1.0 / v;
                            total += (v * v + 1.0) / v
                        }
                    }
                }

                row_sums.iter_mut().for_each(|v| *v /= total);
                row_sums
            }
            WeightMethod::GeometricMean => {
                let mut row_prod = vec![1.0; n];
                let mut total = n as f64; // Por la diagonal que son todos 1

                for i in 0..n {
                    for j in (i + 1)..n {
                        // Recorremos solo la parte superior
                        if let Some(v) = a.get(i, j) {
                            row_prod[i] *= v;
                            row_prod[j] *= 1.0 / v;
                            total += (v * v + 1.0) / v
                        }
                    }
                }

                row_prod.iter_mut().for_each(|v| *v /= total);
                row_prod
            }
            WeightMethod::EigenValues => {
                let mut w = vec![n as f64; n];

                for _ in 0..LIM_IT {
                    // Máximo de seguridad
                    let mut total = 0.0;
                    let mut aw = vec![0.0; n];

                    for i in 0..n {
                        let result = (0..n)
                            .map(|j| a.get(i, j).map_or(0.0, |v| v * w[j]))
                            .sum::<f64>();
                        aw[i] = result;
                        total += result;
                    }

                    aw.iter_mut().for_each(|v| *v /= total); // Normalizar
                    let no_diff = aw
                        .iter()
                        .enumerate()
                        .all(|(i, aw_i)| (w[i] - aw_i).abs() <= TOLERANCE);
                    w = aw;

                    // Si no mejoramos más, paramos porque hemos encontrado el resultado
                    if no_diff {
                        break;
                    }
                }

                w
            }
        }
    }

    pub(crate) fn calc_cicr(&self, weights: &[f64], a: &PairMatrix) -> Option<(f64, f64)> {
        match self {
            WeightMethod::EigenValues => {
                let n = weights.len();
                if n <= 1 {
                    return Some((0.0, 0.0));
                }

                let nf = n as f64;
                let aw = (0..n)
                    .map(|i| {
                        (0..n)
                            .map(|j| a.get(i, j).map_or(0.0, |a_ij| a_ij * weights[j]))
                            .sum()
                    })
                    .collect::<Vec<f64>>();
                let lambda: f64 = aw.iter().sum();
                let ci = (lambda - nf) / (nf - 1.0);
                let ri = RI.get(n - 1).copied().unwrap_or(1.49);

                Some((ci, if ri > 0.0 { ci / ri } else { 0.0 }))
            }
            _ => None,
        }
    }

    pub(crate) fn calc_lambda_max(&self, weights: &[f64], a: &PairMatrix) -> Option<f64> {
        match self {
            WeightMethod::EigenValues => {
                let n = weights.len();
                if n <= 1 {
                    return None;
                }
                let aw = (0..n)
                    .map(|i| {
                        (0..n)
                            .map(|j| a.get(i, j).map_or(0.0, |a_ij| a_ij * weights[j]))
                            .sum()
                    })
                    .collect::<Vec<f64>>();
                Some(aw.iter().sum())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WeightsReport {
    pub weights: Vec<f64>,
    pub lambda_max: Option<f64>,
    pub ci: Option<f64>,
    pub cr: Option<f64>,
}

impl WeightsReport {
    pub(crate) fn from_matrix(method: WeightMethod, m: &PairMatrix) -> Self {
        let weights = method.calc_weights(m);
        let lambda_max = method.calc_lambda_max(&weights, m);
        let (ci, cr) = match method.calc_cicr(&weights, m) {
            Some((ci, cr)) => (Some(ci), Some(cr)),
            None => (None, None),
        };
        Self {
            weights,
            lambda_max,
            ci,
            cr,
        }
    }
}
