#!/bin/bash
# Userdata script for mon2y-trial-daemon
# Variables
SERVICE_USER="mon2y"
APP_DIR="/opt/mon2y/trial_daemon"
VENV_DIR="$APP_DIR/venv"
S3_BUCKET="mon2y"
# This should be replaced with the actual wheel filename or a script to find the latest
DAEMON_WHEEL="mon2y/mon2y_trial_daemon-0.1.0-py3-none-any.whl"

# Install dependencies
dnf install -y python3.13 gcc awscli

# Create user and directories
useradd -r -m -d /home/$SERVICE_USER -s /bin/bash $SERVICE_USER
mkdir -p $APP_DIR
chown -R $SERVICE_USER:$SERVICE_USER /opt/mon2y

# Set up virtual environment
python3.13 -m venv $VENV_DIR
chown -R $SERVICE_USER:$SERVICE_USER $VENV_DIR

# Install wheel
su - $SERVICE_USER -c "aws s3 cp s3://$S3_BUCKET/$DAEMON_WHEEL $APP_DIR/"
su - $SERVICE_USER -c "$VENV_DIR/bin/pip install $APP_DIR/$(basename $DAEMON_WHEEL)"

# Create systemd service file
cat > /etc/systemd/system/mon2y-trial-daemon.service <<EOF
[Unit]
Description=Mon2y Trial Daemon
After=network.target

[Service]
User=$SERVICE_USER
Group=$SERVICE_USER
WorkingDirectory=$APP_DIR
Environment="DIST_URI=http://$DIST_URI:5000"
ExecStart=$VENV_DIR/bin/python -m mon2y_trial_daemon --verbose
Restart=always
SyslogIdentifier=mon2y-trial-daemon

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
systemctl daemon-reload
systemctl enable mon2y-trial-daemon.service
systemctl start mon2y-trial-daemon.service
