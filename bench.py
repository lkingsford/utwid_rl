"""Benchmarks the mon2y rust library's explore function."""

import argparse
import timeit
import statistics
import mon2y

def main():
    """Main function to run the benchmark."""
    parser = argparse.ArgumentParser(
        description="Benchmark the mon2y.explore function."
    )
    parser.add_argument(
        "game",
        type=str,
        choices=["C4", "NT", "CS", "EBR"],
        help="The game to benchmark."
    )
    parser.add_argument(
        "-i", "--iterations",
        type=int,
        default=100000,
        help="Number of iterations per episode."
    )
    parser.add_argument(
        "-t", "--threads",
        type=int,
        default=8,
        help="Number of threads to use."
    )
    parser.add_argument(
        "-e", "--episodes",
        type=int,
        default=10,
        help="Number of episodes to run."
    )
    # player_count is not used by explore, but included for consistency
    parser.add_argument(
        "-p", "--player_count",
        type=int,
        default=3,
        help="Player count for games that require it (currently unused by explore)."
    )
    args = parser.parse_args()

    game_enum = getattr(mon2y.Games, args.game)

    print("===")
    print(f"Iterations: {args.iterations}, Episodes: {args.episodes}, Threads: {args.threads}")
    print("---")

    durations = []
    for i in range(args.episodes):
        print(f"Running episode {i+1}/{args.episodes}...")
        timer = timeit.Timer(
            lambda: mon2y.explore(
                game=game_enum,
                iterations=args.iterations,
                thread_count=args.threads
            )
        )
        # Run one timeit loop per episode
        duration = timer.timeit(number=1)
        durations.append(duration)
        iterations_per_second = args.iterations / duration
        print(
            f"{args.iterations} iterations in {duration:.2f} seconds "
            f"({iterations_per_second:.2f} iterations per second)"
        )

    print("---")
    if durations:
        avg_duration = statistics.mean(durations)
        total_iterations = args.episodes * args.iterations
        total_duration = sum(durations)
        avg_ips = total_iterations / total_duration if total_duration > 0 else 0
        
        print(f"Average duration: {avg_duration:.2f} seconds")
        print(f"Average iterations per second: {avg_ips:.2f}")

if __name__ == "__main__":
    main()
