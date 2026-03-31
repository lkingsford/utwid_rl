pub mod c4;
pub mod cs;
pub mod ebr;
pub mod nt;
pub mod utwid;

pub use c4::C4;
pub use cs::CS;
pub use ebr::EBR;
pub use nt::NT;
pub use utwid::Utwid;

use clap::ValueEnum;
use pyo3::prelude::*;
use serde::Deserialize;

#[pyclass]
#[derive(Debug, Clone, ValueEnum, Deserialize)]
pub enum Games {
    C4,
    NT,
    CS,
    EBR,
    Utwid,
}
