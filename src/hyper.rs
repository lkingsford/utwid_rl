use pyo3::{
    prelude::*,
    types::{PyAny, PyDict},
};
use serde::{de::DeserializeOwned, Serialize};
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
    // Stanza(HashMap<String, ParamMeta>),
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

pub trait Hyperparams: Clone + Send + Sync + Default + DeserializeOwned + 'static {
    fn metadata() -> HashMap<String, ParamMeta>;
    fn from_pydict<'py>(py: Python<'py>, pydict: &Bound<'py, PyDict>) -> PyResult<Self> {
        // Make it work, make it good.
        // This is the alternative to doing a dreserialize of that specific type -
        // it doesn't happen that often, so handling this bit python side.

        let json_module = PyModule::import(py, "json")?;
        let json_str = json_module.call_method1("dumps", (pydict,))?.to_string();

        let hyperparams: Self = serde_json::from_str(&json_str)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(hyperparams)
    }

    /// Convert the meta to a python dictionary
    fn meta_default_pydict(py: Python) -> PyResult<Py<PyAny>> {
        // We're gonna use this in from_pydict with an update_missing.
        // The goal is to get a dictionary of default values, so we first
        // transform the metadata into a map of just the defaults.
        let meta_fields = Self::metadata();
        let defaults: HashMap<String, ParamValue> = meta_fields
            .into_iter()
            .map(|(k, v)| (k, v.default))
            .collect();

        // Now we can use the same serde_json trick as in other functions
        // to convert this map into a Python dictionary.
        let json_str = serde_json::to_string(&defaults)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let json_module = PyModule::import(py, "json")?;
        let py_dict = json_module.call_method1("loads", (json_str,))?;
        Ok(py_dict.into())
    }
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
