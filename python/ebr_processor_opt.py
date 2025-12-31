import os
import time
import multiprocessing as mp

import optuna

import mon2y


CPU_COUNT = os.cpu_count() or 1
MIN_WORK = CPU_COUNT // 2
MAX_WORK = CPU_COUNT * 4

ITERATIONS = 50_000
TIME_LIMIT = 30  # seconds


def worker(args):
    iterations, threads, time_limit = args
    # mon2y.explore returns some iterable-like object
    result = mon2y.explore(
        mon2y.Games.EBR,
        iterations=iterations,
        thread_count=threads,
        time_limit_secs=time_limit,
    )
    return len(result)


def objective(trial: optuna.Trial) -> float:
    processes = trial.suggest_int("processes", 1, CPU_COUNT * 2)
    threads = trial.suggest_int("threads", 1, CPU_COUNT * 2)

    total_workers = processes * threads

    if total_workers < MIN_WORK or total_workers > MAX_WORK:
        raise optuna.TrialPruned()

    start = time.perf_counter()

    with mp.Pool(processes=processes) as pool:
        results = pool.map(
            worker,
            [(ITERATIONS, threads, TIME_LIMIT)] * processes,
        )

    elapsed = time.perf_counter() - start
    total_iterations = sum(results)

    iterations_per_second = total_iterations / elapsed

    # Helpful diagnostics
    trial.set_user_attr("total_iterations", total_iterations)
    trial.set_user_attr("elapsed_seconds", elapsed)

    return iterations_per_second


def main():
    study = optuna.create_study(
        study_name="process_thread_tuning",
        direction="maximize",
        storage="sqlite:///db.sqlite3",
        load_if_exists=True,
    )

    study.optimize(objective, n_trials=50)

    print("\nBest trial:")
    best = study.best_trial
    print(f"  Value (iters/sec): {best.value:.2f}")
    print(f"  Processes: {best.params['processes']}")
    print(f"  Threads:   {best.params['threads']}")
    print(f"  Total workers: {best.params['processes'] * best.params['threads']}")
    print(f"  Total iterations: {best.user_attrs['total_iterations']}")
    print(f"  Elapsed seconds:  {best.user_attrs['elapsed_seconds']:.2f}")


if __name__ == "__main__":
    mp.set_start_method("spawn", force=True)
    main()
