use pyo3::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Uint(u64),
    Bool(bool),
    Enum(String),
    Stanza(HashMap<String, ParamMeta>),
}

#[derive(Clone, Debug)]
pub enum ParamRange {
    FloatRange(f64, f64),
    IntRange(i64, i64),
    UintRange(u64, u64),
    EnumOptions(Vec<String>),
}
#[derive(Clone, Debug)]
pub struct ParamMeta {
    pub default: ParamValue,
    pub range: Option<ParamRange>,
}

pub trait Hyperparams: Clone + Send + Sync + Default + 'static {
    fn metadata() -> HashMap<String, ParamMeta>;
}

pub trait GameHyperrewardTrait: Clone + Send + Sync + Default + 'static {
    fn to_py(&self, py: Python) -> PyResult<Py<PyAny>>;
}

impl GameHyperrewardTrait for () {
    fn to_py(&self, py: Python) -> PyResult<Py<PyAny>> {
        Ok(py.None())
    }
}

impl Hyperparams for () {
    fn metadata() -> HashMap<String, ParamMeta> {
        HashMap::new()
    }
}

#[derive(Clone, Debug)]
pub struct Hyperrewards<T: GameHyperrewardTrait> {
    pub turns: u32,
    pub rwalk: u32,
    pub game_hrs: T,
}

impl<T: GameHyperrewardTrait> Hyperrewards<T> {
    fn random_walk_prop(&self) -> f64 {
        f64::from(self.rwalk) / f64::from(self.turns)
    }
}

pub trait PyHyperrewards: Send + Sync {
    fn turns(&self) -> u32;
    fn rwalk(&self) -> u32;
    fn game_hrs(&self, py: Python) -> PyResult<Py<PyAny>>;
}

impl<T: GameHyperrewardTrait> PyHyperrewards for Hyperrewards<T> {
    fn turns(&self) -> u32 {
        self.turns
    }
    fn rwalk(&self) -> u32 {
        self.rwalk
    }
    fn game_hrs(&self, py: Python) -> PyResult<Py<PyAny>> {
        self.game_hrs.to_py(py)
    }
}

#[pyclass(name = "Hyperrewards")]
pub struct PyHyperrewardsWrapper {
    inner: Box<dyn PyHyperrewards>,
}

impl PyHyperrewardsWrapper {
    pub fn new(inner: Box<dyn PyHyperrewards>) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyHyperrewardsWrapper {
    #[getter]
    pub fn turns(&self) -> u32 {
        self.inner.turns()
    }

    #[getter]
    pub fn rwalk(&self) -> u32 {
        self.inner.rwalk()
    }

    #[getter]
    pub fn game_hrs(&self, py: Python) -> PyResult<Py<PyAny>> {
        self.inner.game_hrs(py)
    }
}
