from .app import app

def main():
    """
    Runs the dashboard application.
    For production, it's recommended to use a proper WSGI server like gunicorn.
    The systemd service file uses gunicorn.
    This entrypoint is for convenience and `python -m` execution.
    """
    app.run_server(debug=False, host='0.0.0.0', port=8050)

if __name__ == '__main__':
    main()
