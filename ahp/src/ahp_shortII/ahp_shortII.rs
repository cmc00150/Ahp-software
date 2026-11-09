use crate::{
    ahp_shortII::profile::Profile,
    share::{
        pair_matrix::PairMatrix,
        pesos::{WeightMethod, WeightsReport},
        quiz::QuizPairMatrix,
        share::Table,
    },
};

pub struct Priorities {
    pub original: f64,
    pub weight: Option<f64>,
    /// Índice de la categoría a la que pertenece el perfil (`None` si es un rango RP).
    pub profile: Option<usize>,
}

pub struct AhpShort {
    criteria_labels: Vec<String>,
    alternatives_labels: Vec<String>,
    categories: Vec<(String, Option<Profile>)>,
    ranges: Vec<f64>,
    values: Table, // Criterios x Alternativas

    criteria_matrix: Option<PairMatrix>,
    ranges_matrix: Option<Vec<PairMatrix>>,

    ranges_priorities: Option<Vec<Vec<Priorities>>>, // valor original, peso
}

impl AhpShort {
    pub fn new(
        c_labels: Vec<impl ToString>,
        alt_labels: Vec<impl ToString>,
        values: Table,
        category_labels: Vec<String>,
        mut profiles: Vec<Profile>,
        ranges: Vec<f64>,
    ) -> Result<Self, &'static str> {
        if alt_labels.len() != values.len() || c_labels.len() != values[0].1.len() {
            Err(
                "[AhpShort::new] La cardinalidad de la matriz de valores no coincide con los vectores de nombes, asegurate de que los criterios son las filas y las alternativas las columnas.",
            )
        } else if Some(category_labels.len()) != profiles.len().checked_sub(1)
            && category_labels.len() != profiles.len()
        {
            Err("[AhpShort::new] Faltan o sobran perfiles con respecto a las categorías.")
        } else if ranges.len() != c_labels.len() {
            Err("[AhpShort::new] Faltan o sobran rangos de los criterios.")
        } else {
            Ok(Self {
                criteria_labels: c_labels.into_iter().map(|l| l.to_string()).collect(),
                alternatives_labels: alt_labels.into_iter().map(|l| l.to_string()).collect(),
                categories: category_labels
                    .into_iter()
                    .map(|l| (l, profiles.pop()))
                    .collect(),
                values,
                ranges,

                ranges_matrix: None,
                criteria_matrix: None,
                ranges_priorities: None,
            })
        }
    }

    pub fn evaluate_criteria(&self) -> QuizPairMatrix {
        QuizPairMatrix::new(&self.criteria_labels, None)
    }

    pub fn evaluate_ranges(&mut self) -> Vec<(String, QuizPairMatrix)> {
        // (Nombre criterio, matriz rangos) y, en paralelo, las prioridades de cada criterio
        let (quizzes, priorities): (Vec<_>, Vec<Vec<Priorities>>) = self
            .criteria_labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let (units, _) = &self.values[i];
                let step = self.ranges[i] / 5.0;

                // Creamos el vector de rangos con los valores equidistantes, le añadimos los valores de los perfiles y por último ordenamos todo (junto con las etiquetas)
                // (etiqueta, valor, categoría del perfil si es un perfil)
                let mut v_ranges = (0..6)
                    .map(|n| (format!("RP{}", n + 1), n as f64 * step, None))
                    .collect::<Vec<(String, f64, Option<usize>)>>();
                v_ranges.extend(self.categories.iter().enumerate().map_while(
                    |(c, (label, profile))| {
                        let profile = profile.as_ref()?;
                        Some((label.clone(), profile.values()[i].data, Some(c)))
                    },
                ));
                v_ranges.sort_by(|a, b| b.1.total_cmp(&a.1));

                let labels = v_ranges.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
                let values = v_ranges.iter().map(|r| r.1).collect::<Vec<_>>();

                // Antes de mover `values` al quiz almacenamos su valor inicial
                let prios = v_ranges
                    .iter()
                    .map(|&(_, original, profile)| Priorities {
                        original,
                        weight: None,
                        profile,
                    })
                    .collect::<Vec<_>>();

                let quiz = QuizPairMatrix::new(&labels, Some(&(units.clone(), values)));
                ((l.clone(), quiz), prios)
            })
            .unzip();

        self.ranges_priorities = Some(priorities);
        quizzes
    }

    pub fn submit_criteria(&mut self, quiz: QuizPairMatrix) -> Result<(), &'static str> {
        if quiz.num_var() != self.criteria_labels.len() {
            Err(
                "[AhpShort::sumbit_criteria] El cuestionario proporcionado no evalua el mismo número de criterios que el sistema.",
            )
        } else {
            self.criteria_matrix = Some(PairMatrix::from_quiz(quiz));
            Ok(())
        }
    }

    pub fn submit_ranges(&mut self, quizs: Vec<QuizPairMatrix>) -> Result<(), &'static str> {
        if quizs.len() != self.criteria_labels.len() {
            return Err(
                "[AhpShort::submit_ranges] El número de cuestionarios no coincide con el de criterios.",
            );
        }
        let priorities = self
            .ranges_priorities
            .as_mut()
            .ok_or("[AhpShort::submit_ranges] Primero hay que llamar a evaluate_ranges.")?;

        // Cada cuestionario debe tener tantas variables como prioridades generó evaluate_ranges
        if quizs
            .iter()
            .zip(priorities.iter())
            .any(|(q, p)| q.num_var() != p.len())
        {
            return Err(
                "[AhpShort::submit_ranges] Algún cuestionario no evalúa el mismo número de rangos y perfiles que el sistema.",
            );
        }

        let matrices: Vec<PairMatrix> = quizs.into_iter().map(PairMatrix::from_quiz).collect();

        // Rellenamos los pesos de las prioridades
        for (row, m) in priorities.iter_mut().zip(&matrices) {
            let w = WeightMethod::EigenValues.calc_weights(m);
            for (prio, wi) in row.iter_mut().zip(w) {
                prio.weight = Some(wi);
            }
        }

        self.ranges_matrix = Some(matrices);
        Ok(())
    }

    pub fn get_matrix_criterion_value(&self, row: usize, col: usize) -> Option<f64> {
        self.criteria_matrix.as_ref().and_then(|m| m.get(row, col))
    }
    pub fn set_matrix_criterion_value(
        &mut self,
        row: usize,
        col: usize,
        value: f64,
    ) -> Result<(), &'static str> {
        self.criteria_matrix
            .as_mut()
            .ok_or(
                "[AhpShort::set_matrix_criterion_value] La matriz de criterios todavía no existe",
            )?
            .set(row, col, value)
    }
    pub fn get_matrix_range_value(
        &self,
        criterion_idx: usize,
        row: usize,
        col: usize,
    ) -> Option<f64> {
        self.ranges_matrix
            .as_ref()? // None si aún no se han creado
            .get(criterion_idx)? // None si el criterio no existe
            .get(row, col)
    }

    pub fn set_matrix_range_value(
        &mut self,
        criterion_idx: usize,
        row: usize,
        col: usize,
        value: f64,
    ) -> Result<(), &'static str> {
        self.ranges_matrix
            .as_mut()
            .ok_or("[AhpShort::set_matrix_alternative_value] Las matrices de rangos todavía no existen")?
            .get_mut(criterion_idx)
            .ok_or("[AhpShort::set_matrix_alternative_value] El criterio no existe")?
            .set(row, col, value)
    }

    pub fn criteria_weights(&self, method: WeightMethod) -> Option<WeightsReport> {
        self.criteria_matrix
            .as_ref()
            .map(|m| WeightsReport::from_matrix(method, m))
    }

    pub fn ranges_weights(
        &self,
        criterion_idx: usize,
        method: WeightMethod,
    ) -> Option<WeightsReport> {
        self.ranges_matrix
            .as_ref()?
            .get(criterion_idx)
            .map(|m| WeightsReport::from_matrix(method, m))
    }

    pub fn ranges_priorities(&self) -> Option<Vec<(&str, &[Priorities])>> {
        self.ranges_priorities.as_ref().map(|rp| {
            self.criteria_labels
                .iter()
                .zip(rp)
                .map(|(label, p)| (label.as_str(), p.as_slice()))
                .collect()
        })
    }

    /// Prioridad local de cada alternativa bajo cada criterio, por interpolación lineal
    /// entre las dos prioridades de rangos más cercanas al valor de la celda:
    ///
    /// `p_kj = p_oj + (p_oj+1 - p_oj) / (s_oj+1 - s_oj) * (g_j(a_k) - s_oj)`
    ///
    /// Devuelve una fila por criterio `(nombre criterio, prioridades por alternativa)`, con la
    /// misma forma que la tabla de valores.
    ///
    /// # Errores
    ///
    /// Si todavía no se han enviado los cuestionarios de rangos (faltan los pesos).
    pub fn calc_alternatives_local_priorities(
        &self,
    ) -> Result<Vec<(String, Vec<f64>)>, &'static str> {
        const MISSING: &str = "[AhpShort::calc_alternatives_local_priorities] Faltan las prioridades de rangos, envía antes los cuestionarios de rangos.";

        let priorities = self.ranges_priorities.as_ref().ok_or(MISSING)?;

        self.criteria_labels
            .iter()
            .zip(&self.values) // Obtenemos un vector ("criterio1", ("unidades", [valores]))
            .zip(priorities) // Obtenemos un vector (("criterio1", ("unidades", [valores])), [prioridades])
            .map(|((label, (_units, cells)), priorities)| {
                // (valor original s, peso p) de cada rango/perfil del criterio
                let points = priorities
                    .iter()
                    .map(|p| p.weight.map(|w| (p.original, w)))
                    .collect::<Option<Vec<(f64, f64)>>>()
                    .ok_or(MISSING)?;

                Ok((
                    label.clone(),
                    cells.iter().map(|&v| interpolate(&points, v)).collect(),
                ))
            })
            .collect()
    }

    /// Clasifica cada alternativa en una categoría.
    ///
    /// 1. Prioridad global de cada alternativa: suma de sus prioridades locales ponderadas por
    ///    el peso de cada criterio.
    /// 2. Prioridad global de cada perfil: suma de las prioridades de sus rangos (el campo
    ///    `weight` de `ranges_priorities` marcado con ese perfil), ponderada igual.
    /// 3. Se ordenan los perfiles de menor a mayor y se obtiene el límite de cada categoría:
    ///    el valor del perfil si es `Limiting`, o el punto medio con el siguiente si es
    ///    `Central`. La última categoría no tiene límite superior. Cada alternativa entra en la
    ///    primera categoría cuyo límite es mayor o igual que su prioridad global.
    ///
    /// Devuelve `(alternativa, prioridad global, categoría)`.
    ///
    /// # Errores
    ///
    /// Si faltan la matriz de criterios o los cuestionarios de rangos.
    pub fn classify(&self) -> Result<Vec<(String, f64, String)>, &'static str> {
        const MISSING: &str =
            "[AhpShort::classify] Faltan la matriz de criterios o las prioridades de rangos.";

        let c_weights = self
            .criteria_weights(WeightMethod::EigenValues)
            .ok_or(MISSING)?
            .weights;
        let priorities = self.ranges_priorities.as_ref().ok_or(MISSING)?;
        let local = self.calc_alternatives_local_priorities()?;

        // 1. Prioridades globales de las alternativas
        let rows = local.iter().map(|(_, c)| c.as_slice()).collect::<Vec<_>>();
        let global = global_priorities(&rows, &c_weights);

        // 2. Prioridades globales de los perfiles: (etiqueta, prioridad, es central)
        let profiles = self
            .categories
            .iter()
            .enumerate()
            .filter_map(|(c, (label, profile))| Some((c, label, profile.as_ref()?)))
            .map(|(c, label, profile)| {
                let g = priorities
                    .iter()
                    .zip(&c_weights)
                    .map(|(row, w)| {
                        row.iter()
                            .find(|p| p.profile == Some(c))
                            .and_then(|p| p.weight)
                            .map(|weight| weight * w)
                    })
                    .sum::<Option<f64>>()
                    .ok_or(MISSING)?;
                Ok((label.as_str(), g, matches!(profile, Profile::Central(_))))
            })
            .collect::<Result<Vec<(&str, f64, bool)>, &'static str>>()?;

        // 3. Límites de cada categoría y asignación
        // Categoría sin perfil (la última): recoge lo que no entra en ninguna de las anteriores
        let catch_all = match self.categories.last() {
            Some((label, None)) => Some(label.as_str()),
            _ => None,
        };
        let limits = category_limits(&profiles, catch_all);

        Ok(self
            .alternatives_labels
            .iter()
            .zip(global)
            .map(|(label, g)| (label.clone(), g, assign_category(&limits, g).to_owned()))
            .collect())
    }
}

/// Prioridad global de cada alternativa: suma de sus prioridades locales ponderada por el peso
/// de cada criterio. `rows[j][a]` es la prioridad local de la alternativa `a` en el criterio `j`.
fn global_priorities(rows: &[&[f64]], weights: &[f64]) -> Vec<f64> {
    let n_alternatives = rows.first().map_or(0, |r| r.len());
    (0..n_alternatives)
        .map(|a| rows.iter().zip(weights).map(|(r, w)| r[a] * w).sum())
        .collect()
}

/// Límite superior de cada categoría, ordenadas de menor a mayor prioridad global.
/// `profiles` son `(categoría, prioridad global, es central)`. Un perfil limitante marca su
/// propio valor y uno central el punto medio con el siguiente. La última categoría no tiene
/// límite superior.
fn category_limits<'a>(
    profiles: &[(&'a str, f64, bool)],
    catch_all: Option<&'a str>,
) -> Vec<(&'a str, f64)> {
    let mut sorted = profiles.to_vec();
    sorted.sort_by(|a, b| a.1.total_cmp(&b.1));

    let mut limits = sorted
        .iter()
        .enumerate()
        .map(|(k, &(label, g, central))| {
            let limit = match (central, sorted.get(k + 1)) {
                (true, Some(&(_, next, _))) => g + (next - g) / 2.0,
                (true, None) => f64::INFINITY,
                (false, _) => g,
            };
            (label, limit)
        })
        .collect::<Vec<_>>();
    if let Some(label) = catch_all {
        limits.push((label, f64::INFINITY));
    }
    if let Some(last) = limits.last_mut() {
        last.1 = f64::INFINITY;
    }
    limits
}

/// Primera categoría cuyo límite es mayor o igual que la prioridad global `g`.
fn assign_category<'a>(limits: &[(&'a str, f64)], g: f64) -> &'a str {
    limits
        .iter()
        .find(|(_, limit)| g <= *limit)
        .map_or("", |(category, _)| category)
}

fn interpolate(points: &[(f64, f64)], v: f64) -> f64 {
    let lower = points
        .iter()
        .filter(|(s, _)| *s <= v) // Nos quedamos con los valores por debajo únicamente, usamos el valor inicial
        .max_by(|a, b| a.0.total_cmp(&b.0)); // Encontramos el valor más alto de los más bajos.
    let upper = points
        .iter()
        .filter(|(s, _)| *s >= v) // Nos quedamos con los valores por encima únicamente
        .min_by(|a, b| a.0.total_cmp(&b.0)); // Encontramos el valor más bajo de los más altos

    match (lower, upper) {
        (Some(&(s_lo, p_lo)), Some(&(s_hi, p_hi))) if s_hi > s_lo => {
            p_lo + (p_hi - p_lo) / (s_hi - s_lo) * (v - s_lo)
        }
        (Some(&(_, p)), _) | (_, Some(&(_, p))) => p, // coincide con un punto o está fuera
        (None, None) => 0.0,                          // sin puntos
    }
}

#[cfg(test)]
mod tests {
    use super::{assign_category, category_limits, global_priorities, interpolate};

    /// Tabla 6 de las diapositivas (su cabecera dice "Robbery", pero sus pesos son los de la
    /// columna Population de la tabla de boroughs): (perfil de referencia `s`, prioridad local `p`).
    const POPULATION: [(f64, f64); 9] = [
        (0.00, 0.320),  // RP1
        (2.00, 0.261),  // CP1
        (5.16, 0.139),  // RP2
        (10.00, 0.075), // CP2
        (10.32, 0.074), // RP3
        (15.48, 0.047), // RP4
        (18.00, 0.032), // CP3
        (20.64, 0.031), // RP5
        (25.80, 0.022), // RP6
    ];

    fn assert_close(obtenido: f64, esperado: f64) {
        assert!(
            (obtenido - esperado).abs() < 1e-3,
            "esperado {esperado}, obtenido {obtenido}"
        );
    }

    #[test]
    fn hackney_del_ejemplo() {
        // 0.139 + (0.075 - 0.139) / (10 - 5.16) * (7.78 - 5.16) = 0.104
        assert_close(interpolate(&POPULATION, 7.78), 0.104);
    }

    #[test]
    fn valor_igual_a_un_punto_devuelve_su_peso() {
        assert_close(interpolate(&POPULATION, 5.16), 0.139);
        assert_close(interpolate(&POPULATION, 0.0), 0.320);
        assert_close(interpolate(&POPULATION, 25.80), 0.022);
    }

    #[test]
    fn fuera_de_rango_usa_el_extremo() {
        assert_close(interpolate(&POPULATION, -3.0), 0.320);
        assert_close(interpolate(&POPULATION, 40.0), 0.022);
    }

    #[test]
    fn no_depende_del_orden_de_los_puntos() {
        let mut desordenados = POPULATION;
        desordenados.reverse();
        assert_close(interpolate(&desordenados, 7.78), 0.104);
    }

    #[test]
    fn sin_puntos_devuelve_cero() {
        assert_close(interpolate(&[], 1.0), 0.0);
    }

    /// Valores iniciales de los 25 primeros boroughs de la hoja de cálculo.
    const ENTRADAS: [f64; 25] = [
        4.82, 6.32, 4.16, 5.70, 5.90, 8.03, 25.80, 5.83, 4.14, 6.46, 5.20, 7.78, 5.94, 7.16, 4.64,
        5.82, 3.59, 3.52, 7.20, 6.76, 4.42, 5.88, 5.03, 5.64, 5.93,
    ];
    /// Columna Population de la tabla de boroughs (Barking and Dagenham ... Newham).
    const ESPERADAS: [f64; 25] = [
        0.152, 0.124, 0.177, 0.132, 0.129, 0.101, 0.022, 0.130, 0.178, 0.122, 0.138, 0.104, 0.129,
        0.113, 0.159, 0.130, 0.199, 0.202, 0.112, 0.118, 0.168, 0.130, 0.144, 0.133, 0.129,
    ];

    #[test]
    fn tabla_population_completa() {
        let errores: Vec<String> = ENTRADAS
            .iter()
            .zip(ESPERADAS)
            .enumerate()
            .filter_map(|(i, (&v, esperado))| {
                let obtenido = interpolate(&POPULATION, v);
                // La hoja redondea a 3 decimales
                ((obtenido - esperado).abs() >= 2e-3).then(|| {
                    format!(
                        "fila {}: g={v} esperado {esperado} obtenido {obtenido:.3}",
                        i + 1
                    )
                })
            })
            .collect();

        assert!(
            errores.is_empty(),
            "{} filas distintas:\n{}",
            errores.len(),
            errores.join("\n")
        );
    }

    // --- classify: datos de la tabla de boroughs de las diapositivas ---

    /// Prioridades globales de los perfiles centrales (columna Global) y su categoría.
    const PERFILES: [(&str, f64, bool); 3] = [
        ("High", 1.754, true),     // CP1
        ("Moderate", 0.516, true), // CP2
        ("Low", 0.261, true),      // CP3
    ];

    #[test]
    fn limites_de_perfiles_centrales() {
        let limits = category_limits(&PERFILES, None);
        // De menor a mayor: Low, Moderate, High
        assert_eq!(limits[0].0, "Low");
        assert_close(limits[0].1, 0.261 + (0.516 - 0.261) / 2.0); // 0.3885
        assert_eq!(limits[1].0, "Moderate");
        assert_close(limits[1].1, 0.516 + (1.754 - 0.516) / 2.0); // 1.135
        assert_eq!(limits[2].0, "High");
        assert!(limits[2].1.is_infinite());
    }

    #[test]
    fn clasifica_los_boroughs_de_la_tabla() {
        let limits = category_limits(&PERFILES, None);
        let esperados = [
            ("Barking and Dagenham", 1.196, "High"),
            ("Barnet", 1.328, "High"),
            ("Bexley", 1.333, "High"),
            ("Brent", 1.224, "High"),
            ("Bromley", 1.324, "High"),
            ("Camden", 1.058, "Moderate"),
            ("City of London Corporation", 0.153, "Low"),
        ];
        for (borough, global, clase) in esperados {
            assert_eq!(assign_category(&limits, global), clase, "{borough}");
        }
    }

    #[test]
    fn perfil_limitante_incluye_su_valor_y_la_categoria_sin_perfil_recoge_el_resto() {
        let profiles = [("Baja", 0.3, false), ("Media", 0.8, false)];
        let limits = category_limits(&profiles, Some("Alta"));
        assert_eq!(assign_category(&limits, 0.3), "Baja"); // igual al límite => esta categoría
        assert_eq!(assign_category(&limits, 0.5), "Media");
        assert_eq!(assign_category(&limits, 0.8), "Media");
        assert_eq!(assign_category(&limits, 5.0), "Alta");
    }

    #[test]
    fn prioridad_global_es_la_suma_ponderada_de_las_locales() {
        // Prioridades locales por criterio (filas) de Barking and Dagenham, Barnet y Camden
        let population: &[f64] = &[0.152, 0.124, 0.101];
        let robbery: &[f64] = &[0.065, 0.069, 0.063];
        let vehicle: &[f64] = &[0.162, 0.219, 0.189];
        let violent: &[f64] = &[0.143, 0.168, 0.111];
        let misbehaviour: &[f64] = &[0.285, 0.287, 0.221];
        let no_force: &[f64] = &[0.187, 0.226, 0.175];
        let drug: &[f64] = &[0.203, 0.236, 0.197];
        let rows = [
            population,
            robbery,
            vehicle,
            violent,
            misbehaviour,
            no_force,
            drug,
        ];

        // En la tabla todos los criterios pesan lo mismo (la global es la suma simple)
        let global = global_priorities(&rows, &[1.0; 7]);
        // Las celdas de la tabla están redondeadas a 3 decimales, de ahí la tolerancia
        for (obtenido, esperado) in global.iter().zip([1.196, 1.328, 1.058]) {
            assert!(
                (obtenido - esperado).abs() < 4e-3,
                "esperado {esperado}, obtenido {obtenido}"
            );
        }

        // Con pesos distintos se pondera de verdad
        let mut pesos = [0.0; 7];
        pesos[1] = 2.0; // solo Robbery, doble
        assert_close(global_priorities(&rows, &pesos)[0], 0.130);
    }
}
