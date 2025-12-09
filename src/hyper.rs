use pyo3::{prelude::*, types::PyAny};
use serde::Serialize;
use serde_json;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Uint(u64),
    Bool(bool),
    Enum(String),
    Stanza(HashMap<String, ParamMeta>),
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ParamRange {
    FloatRange(f64, f64),
    IntRange(i64, i64),
    UintRange(u64, u64),
    EnumOptions(Vec<String>),
}
#[derive(Clone, Debug, Serialize)]
pub struct ParamMeta {
    pub default: ParamValue,
    pub range: Option<ParamRange>,
}

pub trait Hyperparams: Clone + Send + Sync + Default + 'static {
    fn metadata() -> HashMap<String, ParamMeta>;
}

pub trait GameHyperrewardTrait: Clone + Send + Sync + Default + Serialize + 'static {
    fn meta() -> HashMap<String, String>;
}

impl GameHyperrewardTrait for () {
    fn meta() -> HashMap<String, String> {
        HashMap::new()
    }
}

impl Hyperparams for () {
    fn metadata() -> HashMap<String, ParamMeta> {
        HashMap::new()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Hyperrewards<T: GameHyperrewardTrait> {
    pub turns: u32,
    pub rwalk: u32,
    #[serde(flatten)]
    pub game_hrs: T,
}

impl<T: GameHyperrewardTrait> Hyperrewards<T> {
    fn random_walk_prop(&self) -> f64 {
        f64::from(self.rwalk) / f64::from(self.turns)
    }

    pub fn to_py_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let json_str = serde_json::to_string(self)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let json_module = PyModule::import(py, "json")?;
        let py_dict = json_module.call_method1("loads", (json_str,))?;
        Ok(py_dict.into())
    }

    pub fn meta() -> HashMap<String, String> {
        let mut meta = HashMap::from([
            (String::from("turns"), String::from("u32")),
            (String::from("rwalk"), String::from("u32")),
        ]);
        meta.extend(T::meta());
        meta
    }

    pub fn py_meta(py: Python) -> PyResult<Py<PyAny>> {
        let meta = Self::meta();
        let json_str = serde_json::to_string(&meta)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let json_module = PyModule::import(py, "json")?;
        let py_dict = json_module.call_method1("loads", (json_str,))?;
        Ok(py_dict.into())
    }
}
