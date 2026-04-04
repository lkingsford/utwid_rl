pub mod utwid;

pub use utwid::Utwid;

use clap::ValueEnum;
use serde::Deserialize;

#[derive(Debug, Clone, ValueEnum, Deserialize)]
pub enum Games {
    Utwid,
}
