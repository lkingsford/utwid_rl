use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Enum(String),
    Stanza(HashMap<String, ParamMeta>),
}

#[derive(Clone, Debug)]
pub enum ParamRange {
    FloatRange(f64, f64),
    IntRange(i64, i64),
    EnumOptions(Vec<String>),
}
#[derive(Clone, Debug)]
pub struct ParamMeta {
    pub default: ParamValue,
    pub range: Option<ParamRange>,
}

pub trait Hyperparams: Clone + Send + Sync + Default + 'static {
    fn defaults() -> Self;
    fn metadata() -> HashMap<String, ParamMeta>;
}

impl Hyperparams for () {
    fn metadata() -> HashMap<String, ParamMeta> {
        HashMap::new()
    }
    fn defaults() -> Self {}
}
