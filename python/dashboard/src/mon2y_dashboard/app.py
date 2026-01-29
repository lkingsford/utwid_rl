import os
import requests
import dash
from dash import dcc, html
from dash.dependencies import Input, Output
import plotly.graph_objs as go
from collections import deque
import pandas as pd
from datetime import datetime, timedelta

# --- Configuration ---
DIST_URI = os.environ.get("DIST_URI", "http://localhost:5000")
POLL_INTERVAL_SECONDS = 30
TIME_WINDOW_MINUTES = 10

# --- Data Storage ---
MAX_LEN = int(TIME_WINDOW_MINUTES * 60 / POLL_INTERVAL_SECONDS)
timestamps = deque(maxlen=MAX_LEN)
iterations_data = {}
trials_data = {}
processes_data = {}
ask_time_data = {}
overall_ask_time_data = deque(maxlen=MAX_LEN)


# --- Dash App Initialization ---
app = dash.Dash(__name__)

app.layout = html.Div(
    [
        html.H1("Runner Status Dashboard"),
        dcc.Interval(
            id="interval-component",
            interval=POLL_INTERVAL_SECONDS * 1000,  # in milliseconds
            n_intervals=0,
        ),
        html.Div(id="graphs-container"),
    ]
)


# --- Functions ---
def get_runner_status_historical(start_time, end_time):
    """Fetches data from the /runner_status endpoint for a specific time window."""
    try:
        response = requests.get(
            f"{DIST_URI}/runner_status",
            params={
                "start_time": start_time.isoformat(),
                "end_time": end_time.isoformat(),
            },
        )
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching historical data: {e}")
        return None


def get_runner_status_latest():
    """Fetches the latest data from the /runner_status endpoint."""
    try:
        response = requests.get(
            f"{DIST_URI}/runner_status",
            params={"time_seconds": TIME_WINDOW_MINUTES * 60},
        )
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching latest data: {e}")
        return None


def update_data(data, timestamp=None):
    """Updates the data deques with new data for a given timestamp."""
    effective_timestamp = timestamp or datetime.now()
    timestamps.append(effective_timestamp)

    active_runners = set(data.get("runners", {}).keys())
    all_runners = set(iterations_data.keys()) | active_runners

    total_ask_time_in_window = 0
    total_asks_in_window = 0

    for runner in all_runners:
        stats = data.get("runners", {}).get(runner, {})

        if runner not in iterations_data:
            padding = [None] * (len(timestamps) - 1)
            iterations_data[runner] = deque(padding, maxlen=timestamps.maxlen)
            trials_data[runner] = deque(padding, maxlen=timestamps.maxlen)
            processes_data[runner] = deque(padding, maxlen=timestamps.maxlen)
            ask_time_data[runner] = deque(padding, maxlen=timestamps.maxlen)

        iterations_per_min = (stats.get("total_iterations") or 0) / TIME_WINDOW_MINUTES
        trials_per_min = (stats.get("total_trials") or 0) / TIME_WINDOW_MINUTES

        iterations_data[runner].append(iterations_per_min)
        trials_data[runner].append(trials_per_min)
        processes_data[runner].append(stats.get("num_processes"))

        total_ask_time = stats.get("total_ask_time")
        num_asks = stats.get("num_asks")

        if num_asks and num_asks > 0:
            avg_ask_time = total_ask_time / num_asks
            ask_time_data[runner].append(avg_ask_time)
            total_ask_time_in_window += total_ask_time
            total_asks_in_window += num_asks
        else:
            ask_time_data[runner].append(None)

    if total_asks_in_window > 0:
        overall_avg_ask_time = total_ask_time_in_window / total_asks_in_window
        overall_ask_time_data.append(overall_avg_ask_time)
    else:
        overall_ask_time_data.append(None)


def initialize_data():
    """Pre-populates the dashboard with historical data on startup."""
    if iterations_data:  # Ensure this runs only once
        return

    print("Initializing dashboard with historical data...")
    now = datetime.now()
    start_of_window = now - timedelta(minutes=TIME_WINDOW_MINUTES)

    # Create historical poll points from oldest to newest
    historical_poll_points = []
    current_poll_point = start_of_window
    while current_poll_point <= now:
        historical_poll_points.append(current_poll_point)
        current_poll_point += timedelta(seconds=POLL_INTERVAL_SECONDS)

    for poll_time in historical_poll_points:
        window_end = poll_time
        window_start = window_end - timedelta(seconds=30)

        data = get_runner_status_historical(window_start, window_end)
        if data:
            update_data(data, timestamp=poll_time)
    print("Historical data initialization complete.")


def create_figure(data_dict, title, yaxis_title="Count"):
    """Creates a Plotly figure for the given data."""
    fig = go.Figure()
    for runner, data in data_dict.items():
        if not timestamps or not data:
            continue

        df = pd.DataFrame({"timestamp": list(timestamps), "value": list(data)})
        df["timestamp"] = pd.to_datetime(df["timestamp"])
        df = df.set_index("timestamp")

        # 1-minute running average
        df_1min = df.rolling("1min").mean()
        fig.add_trace(
            go.Scatter(
                x=df_1min.index,
                y=df_1min["value"],
                mode="lines",
                name=f"{runner} (1 min avg)",
            )
        )

        # 10-minute running average
        df_10min = df.rolling("10min").mean()
        fig.add_trace(
            go.Scatter(
                x=df_10min.index,
                y=df_10min["value"],
                mode="lines",
                name=f"{runner} (10 min avg)",
                line=dict(dash="dash"),
            )
        )

    fig.update_layout(
        title=title, xaxis_title="Time", yaxis_title=yaxis_title, showlegend=True
    )
    return fig


def create_single_figure(data_deque, title, yaxis_title="Count"):
    """Creates a Plotly figure for a single data series."""
    fig = go.Figure()
    if not timestamps or not data_deque:
        return fig

    df = pd.DataFrame({"timestamp": list(timestamps), "value": list(data_deque)})
    df["timestamp"] = pd.to_datetime(df["timestamp"])
    df = df.set_index("timestamp")

    # 1-minute running average
    df_1min = df.rolling("1min").mean()
    fig.add_trace(
        go.Scatter(x=df_1min.index, y=df_1min["value"], mode="lines", name="1 min avg")
    )

    # 10-minute running average
    df_10min = df.rolling("10min").mean()
    fig.add_trace(
        go.Scatter(
            x=df_10min.index,
            y=df_10min["value"],
            mode="lines",
            name="10 min avg",
            line=dict(dash="dash"),
        )
    )

    fig.update_layout(
        title=title, xaxis_title="Time", yaxis_title=yaxis_title, showlegend=True
    )
    return fig


# --- Initialization and Callback ---
initialize_data()


@app.callback(
    Output("graphs-container", "children"), [Input("interval-component", "n_intervals")]
)
def update_graphs(n):
    """Callback to update all graphs."""
    # Only fetch new data in the callback, initialization is done
    if n > 0:
        data = get_runner_status_latest()
        if data:
            update_data(data)

    if not iterations_data:
        return html.Div("No data available. Waiting for the first poll...")

    graphs = [
        dcc.Graph(
            id="iterations-graph",
            figure=create_figure(
                iterations_data, "Average Iterations per Minute", "Iterations/min"
            ),
        ),
        dcc.Graph(
            id="trials-graph",
            figure=create_figure(
                trials_data, "Average Trials per Minute", "Trials/min"
            ),
        ),
        dcc.Graph(
            id="processes-graph",
            figure=create_figure(processes_data, "Active Processes", "Count"),
        ),
        dcc.Graph(
            id="ask-time-graph",
            figure=create_figure(
                ask_time_data, "Average Ask Time per Runner", "Seconds"
            ),
        ),
        dcc.Graph(
            id="overall-ask-time-graph",
            figure=create_single_figure(
                overall_ask_time_data, "Overall Average Ask Time", "Seconds"
            ),
        ),
    ]

    return graphs


# --- Main ---
if __name__ == "__main__":
    app.run(debug=True, port=8050)

