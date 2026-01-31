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
DIST_STATUS_MAX_LEN = 100
GRAPH_BG_COLOR = "rgba(240, 240, 240, 0.95)"

# --- Color Management for PIDs ---
PID_COLORS = {}
AVAILABLE_COLORS = [
    "#1f77b4",
    "#ff7f0e",
    "#2ca02c",
    "#d62728",
    "#9467bd",
    "#8c564b",
    "#e377c2",
    "#7f7f7f",
    "#bcbd22",
    "#17becf",
]
COLOR_INDEX = 0


def get_pid_color(pid):
    global COLOR_INDEX
    if pid not in PID_COLORS:
        PID_COLORS[pid] = AVAILABLE_COLORS[COLOR_INDEX % len(AVAILABLE_COLORS)]
        COLOR_INDEX += 1
    return PID_COLORS[pid]


# --- Data Storage ---
# Runner Stats (new simplified structure)
runner_data = {}  # Store timeseries data directly: {runner_id: [{"timestamp": ts, ...}]}

# Distribution Stats
dist_timestamps = deque(maxlen=DIST_STATUS_MAX_LEN)
cpu_data = deque(maxlen=DIST_STATUS_MAX_LEN)
ram_data = deque(maxlen=DIST_STATUS_MAX_LEN)
queue_size_data = {}
ops_calls_data = {}
dropped_ops_data = {}


# --- Dash App Initialization ---
app = dash.Dash(__name__)

app.layout = html.Div(
    [
        html.H1("Runner and Distribution Status Dashboard"),
        dcc.Interval(
            id="interval-component",
            interval=POLL_INTERVAL_SECONDS * 1000,
            n_intervals=0,
        ),
        html.Div(id="graphs-container"),
    ]
)


# --- Data Fetching Functions ---
def get_runner_status_timeseries(start_time, end_time):
    try:
        response = requests.get(
            f"{DIST_URI}/runner_status_timeseries",
            params={
                "start_time": start_time.isoformat(),
                "end_time": end_time.isoformat(),
                "bucket_seconds": 10,  # Match the 10s suggestion
            },
        )
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching runner timeseries data: {e}")
        return None


def get_dist_status(entries=1):
    try:
        response = requests.get(f"{DIST_URI}/dist_status", params={"entries": entries})
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching dist status data: {e}")
        return None


# --- Data Processing Functions ---
def update_runner_data(data):
    """
    Processes the timeseries data from the /runner_status_timeseries endpoint.
    """
    global runner_data
    # The new API response is the source of truth
    runners_ts_data = data.get("runners", {})
    
    # Convert timestamps from ISO strings to datetime objects
    for runner_id, points in runners_ts_data.items():
        for point in points:
            point["timestamp"] = datetime.fromisoformat(point["timestamp"])
    
    runner_data = runners_ts_data


def update_dist_data(data):
    # This function now handles a list of entries for initialization
    server_stats_list = data.get("server_stats", [])
    process_stats_list = data.get("process_stats", [])

    # Group process stats by timestamp
    proc_by_ts = {}
    for p in process_stats_list:
        proc_by_ts.setdefault(p["timestamp"], []).append(p)

    for server_entry in server_stats_list:
        ts_str = server_entry["timestamp"]
        ts = datetime.fromisoformat(ts_str)
        dist_timestamps.append(ts)

        cpu_data.append(server_entry.get("cpu_usage_percent"))
        ram_data.append(server_entry.get("ram_usage_percent"))

        # Get all process stats for this timestamp
        process_stats_for_ts = proc_by_ts.get(ts_str, [])
        active_pids_for_ts = {p["pid"] for p in process_stats_for_ts}

        all_known_pids = set(queue_size_data.keys()) | active_pids_for_ts

        for pid in all_known_pids:
            if pid not in queue_size_data:
                # Add padding for new PIDs to match existing timestamp length
                padding = [None] * (len(dist_timestamps) - 1)
                queue_size_data[pid] = deque(padding, maxlen=DIST_STATUS_MAX_LEN)
                ops_calls_data[pid] = deque(padding, maxlen=DIST_STATUS_MAX_LEN)
                dropped_ops_data[pid] = deque(padding, maxlen=DIST_STATUS_MAX_LEN)

            if pid in active_pids_for_ts:
                stat = next(
                    (p for p in process_stats_for_ts if p.get("pid") == pid), {}
                )
                queue_size_data[pid].append(stat.get("ops_queue_size"))
                ops_calls_data[pid].append(stat.get("ops_calls_last_minute"))
                dropped_ops_data[pid].append(stat.get("dropped_ops_last_minute"))
            else:
                # If a known PID has no data for this timestamp, append None
                queue_size_data[pid].append(None)
                ops_calls_data[pid].append(None)
                dropped_ops_data[pid].append(None)


# --- Historical Data Initialization ---
def initialize_runner_data():
    if runner_data:
        return
    print("Initializing runner stats with historical data...")
    now = datetime.now()
    start_of_window = now - timedelta(minutes=TIME_WINDOW_MINUTES)
    data = get_runner_status_timeseries(start_of_window, now)
    if data:
        update_runner_data(data)
    print("Runner stats initialization complete.")


def initialize_dist_data():
    if cpu_data:
        return
    print("Initializing distribution stats with historical data...")
    data = get_dist_status(entries=DIST_STATUS_MAX_LEN)
    if not data:
        return
    # Data is newest first, reverse to process oldest first
    if "server_stats" in data:
        data["server_stats"].reverse()
    if "process_stats" in data:
        data["process_stats"].reverse()
    update_dist_data(data)
    print("Distribution stats initialization complete.")


# --- Graph Creation Functions ---
def create_runner_figure(metric_key, title, yaxis_title):
    fig = go.Figure()
    for runner_id, points in runner_data.items():
        if not points:
            continue
        timestamps = [p["timestamp"] for p in points]
        values = [p[metric_key] for p in points]
        fig.add_trace(
            go.Scatter(x=timestamps, y=values, mode="lines", name=runner_id)
        )
    fig.update_layout(
        title=title, xaxis_title="Time", yaxis_title=yaxis_title, showlegend=True
    )
    return fig


def create_dist_figure(data_dict, title, yaxis_title):
    fig = go.Figure()
    for pid, data in data_dict.items():
        if not dist_timestamps or not data:
            continue
        df = pd.DataFrame(
            {"timestamp": list(dist_timestamps), "value": list(data)}
        ).set_index("timestamp")
        df_1min = df.rolling("1min").mean()
        fig.add_trace(
            go.Scatter(
                x=df_1min.index,
                y=df_1min["value"],
                mode="lines",
                name=f"PID: {pid}",
                line=dict(color=get_pid_color(pid)),
            )
        )
    fig.update_layout(
        title=title,
        xaxis_title="Time",
        yaxis_title=yaxis_title,
        showlegend=True,
        plot_bgcolor=GRAPH_BG_COLOR,
        paper_bgcolor=GRAPH_BG_COLOR,
    )
    return fig


def create_combined_ops_figure():
    fig = go.Figure()
    for pid in ops_calls_data.keys():
        if not dist_timestamps:
            continue
        calls_df = pd.DataFrame(
            {
                "timestamp": list(dist_timestamps),
                "value": list(ops_calls_data.get(pid, [])),
            }
        ).set_index("timestamp")
        drops_df = pd.DataFrame(
            {
                "timestamp": list(dist_timestamps),
                "value": list(dropped_ops_data.get(pid, [])),
            }
        ).set_index("timestamp")

        calls_1min = calls_df.rolling("1min").mean()
        drops_1min = drops_df.rolling("1min").mean()

        pid_color = get_pid_color(pid)
        fig.add_trace(
            go.Scatter(
                x=calls_1min.index,
                y=calls_1min["value"],
                mode="lines",
                name=f"Calls PID: {pid}",
                line=dict(color=pid_color),
            )
        )
        fig.add_trace(
            go.Scatter(
                x=drops_1min.index,
                y=drops_1min["value"],
                mode="lines",
                name=f"Drops PID: {pid}",
                line=dict(dash="dash", color=pid_color),
            )
        )
    fig.update_layout(
        title="Ops Calls vs. Drops (per Minute)",
        xaxis_title="Time",
        yaxis_title="Count",
        showlegend=True,
        plot_bgcolor=GRAPH_BG_COLOR,
        paper_bgcolor=GRAPH_BG_COLOR,
    )
    return fig


def create_single_series_figure(
    data_deque, ts_deque, title, yaxis_title, use_runner_ts=True
):
    fig = go.Figure()
    if not ts_deque or not data_deque:
        return fig
    df = pd.DataFrame(
        {"timestamp": list(ts_deque), "value": list(data_deque)}
    ).set_index("timestamp")
    df_1min = df.rolling("1min").mean()
    fig.add_trace(
        go.Scatter(x=df_1min.index, y=df_1min["value"], mode="lines", name="1 min avg")
    )
    bg_color = GRAPH_BG_COLOR if not use_runner_ts else "rgba(255,255,255,0)"
    fig.update_layout(
        title=title,
        xaxis_title="Time",
        yaxis_title=yaxis_title,
        showlegend=True,
        plot_bgcolor=bg_color,
        paper_bgcolor=bg_color,
    )
    return fig


# --- Initialization and Callback ---
initialize_runner_data()
initialize_dist_data()


@app.callback(
    Output("graphs-container", "children"), [Input("interval-component", "n_intervals")]
)
def update_graphs(n):
    # Fetch fresh data on every poll
    now = datetime.now()
    start_of_window = now - timedelta(minutes=TIME_WINDOW_MINUTES)
    runner_ts_data = get_runner_status_timeseries(start_of_window, now)
    if runner_ts_data:
        update_runner_data(runner_ts_data)

    # Dist data is polled less frequently and uses its own logic
    if n > 0: # Only poll dist status after initial load
        dist_data = get_dist_status()
        if dist_data:
            update_dist_data(dist_data)

    if not runner_data and not cpu_data:
        return html.Div("No data available. Waiting for the first poll...")

    runner_graphs = [
        dcc.Graph(
            id="iterations-graph",
            figure=create_runner_figure(
                "iterations_per_minute", "Iterations per Minute", "Iterations/min"
            ),
        ),
        dcc.Graph(
            id="trials-graph",
            figure=create_runner_figure(
                "trials_per_minute", "Trials per Minute", "Trials/min"
            ),
        ),
        dcc.Graph(
            id="processes-graph",
            figure=create_runner_figure(
                "num_processes", "Active Runner Processes", "Count"
            ),
        ),
        dcc.Graph(
            id="ask-time-graph",
            figure=create_runner_figure(
                "avg_ask_time_s", "Average Ask Time per Runner", "Seconds"
            ),
        ),
    ]

    dist_graphs = [
        html.Hr(),
        dcc.Graph(
            id="cpu-graph",
            figure=create_single_series_figure(
                cpu_data, dist_timestamps, "CPU Usage", "%", use_runner_ts=False
            ),
        ),
        dcc.Graph(
            id="ram-graph",
            figure=create_single_series_figure(
                ram_data, dist_timestamps, "RAM Usage", "%", use_runner_ts=False
            ),
        ),
        dcc.Graph(
            id="queue-size-graph",
            figure=create_dist_figure(queue_size_data, "Operation Queue Size", "Count"),
        ),
        dcc.Graph(id="ops-graph", figure=create_combined_ops_figure()),
    ]

    return runner_graphs + dist_graphs


# --- Main ---
if __name__ == "__main__":
    app.run(debug=True, port=8050)
