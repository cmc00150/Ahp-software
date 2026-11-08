#[derive(Debug, Clone)]
pub struct Value {
    pub data: f64,
    pub unit: String,
}

pub type RowWithUnits = (String, Vec<f64>);
pub type Table = Vec<RowWithUnits>;
