# Runner Status Dashboard

This dashboard visualizes the status of the runners from the `mon2y_dist` application.

## Installation

1.  Install the required Python packages:
    ```bash
    pip install -r requirements.txt
    ```

## Running the Dashboard

1.  Make sure the `mon2y_dist` application is running and accessible.
2.  Set the `DIST_URI` environment variable if the `mon2y_dist` application is not running on `http://localhost:5000`.
    ```bash
    export DIST_URI="http://<your-dist-host>:<port>"
    ```
3.  Run the dashboard application:
    ```bash
    python app.py
    ```
4.  Open your web browser and navigate to `http://127.0.0.1:8050`.
