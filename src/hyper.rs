use pyo3::{
    prelude::*,
    types::{PyAny, PyDict},
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json;
use std::collections::HashMap;

pub trait Hyperparams: Clone + Send + Sync + Default + DeserializeOwned + 'static {
    fn from_pydict<'py>(py: Python<'py>, pydict: &Bound<'py, PyDict>) -> PyResult<Self> {
        let json_module = PyModule::import(py, "json")?;
        let json_str = json_module.call_method1("dumps", (pydict,))?.to_string();

        serde_json::from_str(&json_str)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
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
    fn from_pydict<'py>(_py: Python<'py>, pydict: &Bound<'py, PyDict>) -> PyResult<Self> {
        if pydict.is_empty() {
            Ok(())
        } else {
            Err(pyo3::exceptions::PyValueError::new_err(
                "Received non-empty hyperparams for a game that doesn't take any.",
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Hyperrewards<T: GameHyperrewardTrait> {
    pub turns: u32,
    pub rwalk: u32,
    pub sum_diff_est_reward: f64,
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