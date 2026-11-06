use ahp_classic::{
    self,
    estructuras::{PairMatrix, QuizPairMatrix},
    pesos::WeightMethod,
};

fn main() {
    println!("Tabla a calcular:");
    print!(
        "[1,   6, 3  ],\n
         [1/6, 1, 1/2],\n
         [1/3, 2, 1  ]\n"
    );

    let mut q = QuizPairMatrix::new(vec!["c1".to_string(), "c2".to_string(), "c3".to_string()]);
    q.set_item(0, 6.0);
    q.set_item(1, 3.0);
    q.set_item(2, 1.0 / 2.0);

    let m = PairMatrix::from_quiz(q);
    let v = WeightMethod::EigenValues.calc_weights(&m);
    println!("Autovector: {:?}", v);
    println!(
        "Autovalor: {:?}",
        WeightMethod::EigenValues.calc_lambda_max(&v, &m)
    );
    let (ci, cr) = WeightMethod::EigenValues
        .calc_CICR(&v, &m)
        .map_or_default(|_| (-1.0, -1.0));
    println!("CI: {} y CR: {}", ci, cr)
}
