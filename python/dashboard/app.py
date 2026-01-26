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

# --- Dash App Initialization ---
app = dash.Dash(__name__)

app.layout = html.Div([
    html.H1("Runner Status Dashboard"),
    dcc.Interval(
        id='interval-component',
        interval=POLL_INTERVAL_SECONDS * 1000,  # in milliseconds
        n_intervals=0
    ),
    html.Div(id='graphs-container')
])

# --- Functions ---
def get_runner_status():
    """Fetches data from the /runner_status endpoint."""
    try:
        response = requests.get(f"{DIST_URI}/runner_status", params={"time_seconds": TIME_WINDOW_MINUTES * 60})
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching data: {e}")
        return None

def update_data(data):
    """Updates the data deques with new data."""
    now = datetime.now()
    timestamps.append(now)

    active_runners = set(data.get('runners', {}).keys())
    all_runners = set(iterations_data.keys()) | active_runners

    for runner in all_runners:
        stats = data.get('runners', {}).get(runner, {})
        
        if runner not in iterations_data:
            padding = [None] * (len(timestamps) - 1)
            iterations_data[runner] = deque(padding, maxlen=timestamps.maxlen)
            trials_data[runner] = deque(padding, maxlen=timestamps.maxlen)
            processes_data[runner] = deque(padding, maxlen=timestamps.maxlen)

        iterations_data[runner].append(stats.get('total_iterations'))
        trials_data[runner].append(stats.get('total_trials'))
        processes_data[runner].append(stats.get('num_processes'))


def create_figure(data_dict, title):
    """Creates a Plotly figure for the given data."""
    fig = go.Figure()
    for runner, data in data_dict.items():
        if not timestamps or not data:
            continue

        df = pd.DataFrame({'timestamp': list(timestamps), 'value': list(data)})
        df['timestamp'] = pd.to_datetime(df['timestamp'])
        df = df.set_index('timestamp')

        # 1-minute running average
        df_1min = df.rolling('1min').mean()
        fig.add_trace(go.Scatter(
            x=df_1min.index,
            y=df_1min['value'],
            mode='lines',
            name=f'{runner} (1 min avg)'
        ))

        # 10-minute running average
        df_10min = df.rolling('10min').mean()
        fig.add_trace(go.Scatter(
            x=df_10min.index,
            y=df_10min['value'],
            mode='lines',
            name=f'{runner} (10 min avg)',
            line=dict(dash='dash')
        ))

    fig.update_layout(
        title=title,
        xaxis_title="Time",
        yaxis_title="Count",
        showlegend=True
    )
    return fig

# --- Callback ---
@app.callback(
    Output('graphs-container', 'children'),
    [Input('interval-component', 'n_intervals')]
)
def update_graphs(n):
    """Callback to update all graphs."""
    data = get_runner_status()
    if data:
        update_data(data)

    if not iterations_data:
        return html.Div("No data yet. Waiting for the first poll...")

    graphs = [
        dcc.Graph(id='iterations-graph', figure=create_figure(iterations_data, "Iterations per Minute")),
        dcc.Graph(id='trials-graph', figure=create_figure(trials_data, "Trials per Minute")),
        dcc.Graph(id='processes-graph', figure=create_figure(processes_data, "Number of Processes"))
    ]

    return graphs

# --- Main ---
if __name__ == '__main__':
    app.run(debug=True, port=8050)
