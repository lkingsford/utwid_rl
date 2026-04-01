# Distributed Operation

## Main entry points

- Rust library / Python extension:
  - `Cargo.toml` exposes `[lib] name = "mon2y"` from `src/lib.rs`.
  - This is the PyO3 module imported by `python/mon2y/__init__.py`.
  - Main exported Python-facing functions are `explore(...)`, `get_hyperreward_meta(...)`, `default_hyperparams(...)`, and `set_log_level(...)`.

- Rust binaries:
  - `src/main.rs`: interactive/local game runner for `c4`, `nt`, `cs`, `ebr`, `utwid`.
  - `src/bench.rs`: benchmark runner.
  - `src/arena.rs`: extra Rust binary in the repo, but not part of the distributed HTTP flow.
  - `src/utwid_auto.rs`: local terminal automation for `utwid`, not part of distributed serving.

- Python package `mon2y`:
  - `python/mon2y` is the Python wrapper around the Rust extension.
  - `python/mon2y/ebr_opt.py` contains the distributed worker function `trial_worker(...)`.
  - In distributed mode, trial runner subprocesses execute `mon2y.ebr_opt.trial_worker(...)`.

- Python package `mon2y-dist`:
  - Built from `python/mon2y_dist`.
  - Console entry point: `mon2y-dist = mon2y_dist.main:main`.
  - Runs the Flask app that coordinates studies and Optuna trial allocation/results.
  - In production userdata it is started via Gunicorn on `0.0.0.0:5000`.

- Python package `mon2y-trial-daemon`:
  - Built from `python/mon2y_trial_daemon`.
  - Console entry point: `mon2y-trial-daemon = mon2y_trial_daemon.main:main`.
  - Polls the dist server for open studies, downloads the correct wheel for the machine architecture, and spawns worker subprocesses.

- `python/ask_serve.py`:
  - Stub / incomplete file.
  - It does not currently define the live HTTP service path used by the deployment scripts.

## Tags and build paths

- `x-*` tags:
  - Trigger `.github/workflows/build-wheels.yml`.
  - Build the Rust/PyO3 wheel from the repo root via `maturin`.
  - Targets: Linux `x86_64` + `aarch64`, macOS `x86_64` + `aarch64`, Windows `x86_64`.
  - Wheel name is normalized from the Rust `[lib]` name `mon2y`.
  - Built wheel filenames get a `.dev<run_number>` suffix and are uploaded to `s3://mon2y/wheels/...`.

- `main` branch pushes:
  - Also trigger `.github/workflows/build-wheels.yml`.
  - Same Rust/PyO3 wheel build/upload path as `x-*`.

- `d-*` tags:
  - Trigger `.github/workflows/build-python-wheels.yml` `build_dist_wheel`.
  - Build the pure-Python `mon2y-dist` wheel from `python/mon2y_dist`.
  - Upload to `s3://mon2y/mon2y/...`.

- `t-*` tags:
  - Trigger `.github/workflows/build-python-wheels.yml` `build_trial_daemon_wheel`.
  - Build the pure-Python `mon2y-trial-daemon` wheel from `python/mon2y_trial_daemon`.
  - Upload to `s3://mon2y/mon2y/...`.

## Runtime wiring

- Dist server machine:
  - `aws/userdata_dist.sh` installs PostgreSQL and `mon2y-dist`.
  - Starts Gunicorn with `mon2y_dist.main:app` on port `5000`.
  - Optuna storage is intended to be local PostgreSQL on the same machine.

- Trial daemon machine:
  - `aws/userdata_trial_daemon.sh` installs `mon2y-trial-daemon`.
  - Sets `DIST_URI=http://<dist-host>:5000`.
  - The daemon polls the dist server and launches worker subprocesses.

- Study control helpers:
  - `create_ex.sh` posts to `/create_study`.
  - `start_ex.sh` posts `/set_status` with `open`.
  - `stop_ex.sh` posts `/set_status` with `done`.

- Local `systemd/*.service` files:
  - These look like older/local examples rather than the current packaged deployment path.
  - The userdata scripts are the clearer source of truth for the current dist/trial-daemon deployment.

## HTTP surface

- Served by `mon2y_dist.main:app` on port `5000`:
  - `POST /create_study`: create/load a study, set module/function metadata, attach wheel locations, set iterations/status.
  - `POST /set_status`: mark a study `open` or `done`.
  - `POST /ask`: hand out the next Optuna trial suggestion.
  - `POST /tell`: accept trial results back from workers.
  - `POST /heartbeat`: record Optuna heartbeat for a running trial.
  - `POST /update_wheel`: upload a replacement wheel and attach it to a study.
  - `POST /remove_wheel`: clear the attached wheel for a study/platform.
  - `GET /open`: list open studies, including presigned wheel download URLs.
  - `GET /runner_status_timeseries`: runner status history.
  - `GET /runner_status`: current/recent runner status summary.
  - `GET /dist_status`: server/process stats summary.

- Main caller pattern:
  - Trial daemon calls `GET /open`.
  - Worker subprocesses in `python/mon2y/ebr_opt.py` call `POST /ask` then `POST /tell`.
  - Manual/operator scripts call `POST /create_study` and `POST /set_status`.

## Ports / SSH forwarding

- Required network listener:
  - `5000/tcp` on the dist server for the Flask/Gunicorn API.

- Intended exposure model:
  - This service is designed to stay private.
  - Forward `5000` over SSH when operating from your workstation instead of opening it publicly.

- Typical forwards:
  - Local operator access to the dist API:
    - `ssh -L 5000:127.0.0.1:5000 <dist-host>`
  - If the dist service is bound on the private interface rather than loopback:
    - `ssh -L 5000:<dist-private-ip>:5000 <bastion-or-dist-host>`

- Practical note:
  - The checked-in local `create_ex.sh`, `start_ex.sh`, and `stop_ex.sh` all target `http://127.0.0.1:5000/...`, so local port-forwarding that lands on remote port `5000` is the simplest operator setup.
