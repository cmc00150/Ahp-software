use crate::share::share::Value;

pub enum Profile {
    Limiting(Vec<Value>),
    Central(Vec<Value>),
}

impl Profile {
    pub fn values(&self) -> &[Value] {
        match self {
            Profile::Limiting(v) | Profile::Central(v) => v,
        }
    }
}
