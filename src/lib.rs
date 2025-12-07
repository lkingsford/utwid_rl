use crate::{
    game::Game,
    games::{c4::C4Hyperparams, c4::C4, cs::CS, ebr::EBR, nt::NT, Games},
    hyper::Hyperparams,
    mcts::mcts::explore_tree,
};
use log::LevelFilter;
use pyo3::{prelude::*, types::PyAny, types::PyDict};
use std::str::FromStr;

pub mod game;
pub mod games;
pub mod hyper;
pub mod mcts;
pub mod test;

#[pyfunction]
fn set_log_level(level: &str) -> PyResult<()> {
    let log_level = LevelFilter::from_str(level)
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Invalid log level"))?;

    log::set_max_level(log_level);
    Ok(())
}

#[pyfunction]
#[pyo3(
    signature = (game, iterations, thread_count, time_limit_secs = None, exploration_constant = None, hyperparams = None)
)]
fn explore(
    py: Python,
    game: Games,
    iterations: usize,
    thread_count: usize,
    time_limit_secs: Option<u64>,
    exploration_constant: Option<f64>,
    hyperparams: Option<&Bound<PyDict>>,
) -> PyResult<Vec<Py<PyAny>>> {
    let time_limit = time_limit_secs.map(std::time::Duration::from_secs);

    let exploration_constant = match exploration_constant {
        None => 2.0_f64.sqrt(),
        Some(constant) => constant,
    };

    let results = match game {
        Games::C4 => {
            let game = C4;
            let h_params = if let Some(py_dict) = hyperparams {
                let mut hp = C4Hyperparams::default();
                if let Some(width) = py_dict.get_item("board_width")? {
                    hp.board_width = width.extract()?;
                }
                if let Some(height) = py_dict.get_item("board_height")? {
                    hp.board_height = height.extract()?;
                }
                hp
            } else {
                C4Hyperparams::default()
            };
            let state = game.init_game(&h_params);
            explore_tree(
                iterations,
                time_limit,
                thread_count,
                state,
                exploration_constant,
            )
            .into_iter()
            .map(|r| r.to_py_dict(py))
            .collect::<PyResult<Vec<_>>>()?
        }
        Games::NT => {
            let game = NT { player_count: 2 };
            let hyperparams = ();
            let state = game.init_game(&hyperparams);
            explore_tree(
                iterations,
                time_limit,
                thread_count,
                state,
                exploration_constant,
            )
            .into_iter()
            .map(|r| r.to_py_dict(py))
            .collect::<PyResult<Vec<_>>>()?
        }
        Games::CS => {
            let game = CS { player_count: 2 };
            let hyperparams = ();
            let state = game.init_game(&hyperparams);
            explore_tree(
                iterations,
                time_limit,
                thread_count,
                state,
                exploration_constant,
            )
            .into_iter()
            .map(|r| r.to_py_dict(py))
            .collect::<PyResult<Vec<_>>>()?
        }
        Games::EBR => {
            let game = EBR { player_count: 2 };
            let hyperparams = ();
            let state = game.init_game(&hyperparams);
            explore_tree(
                iterations,
                time_limit,
                thread_count,
                state,
                exploration_constant,
            )
            .into_iter()
            .map(|r| r.to_py_dict(py))
            .collect::<PyResult<Vec<_>>>()?
        }
    };

    Ok(results)
}

#[pyfunction]
fn get_hyperreward_meta(py: Python, game: Games) -> PyResult<Py<PyAny>> {
    match game {
        Games::C4 => hyper::Hyperrewards::<games::c4::C4Hyperrewards>::py_meta(py),
        Games::NT => hyper::Hyperrewards::<()>::py_meta(py),
        Games::CS => hyper::Hyperrewards::<()>::py_meta(py),
        Games::EBR => hyper::Hyperrewards::<()>::py_meta(py),
    }
}

#[pyfunction]
fn get_hyperparam_meta(py: Python, game: Games) -> PyResult<Py<PyAny>> {
    let meta = match game {
        Games::C4 => C4Hyperparams::metadata(),
        Games::NT => <() as Hyperparams>::metadata(),
        Games::CS => <() as Hyperparams>::metadata(),
        Games::EBR => <() as Hyperparams>::metadata(),
    };

    let json_str = serde_json::to_string(&meta)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    let json = PyModule::import_bound(py, "json")?;
    let py_dict = json.call_method1("loads", (json_str,))?;
    Ok(py_dict.to_object(py))
}

#[pymodule]
fn mon2y(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    m.add_class::<Games>()?;
    m.add_function(wrap_pyfunction!(explore, m)?)?;
    m.add_function(wrap_pyfunction!(get_hyperreward_meta, m)?)?;
    m.add_function(wrap_pyfunction!(get_hyperparam_meta, m)?)?;
    m.add_function(wrap_pyfunction!(set_log_level, m)?)?;
    Ok(())
}
