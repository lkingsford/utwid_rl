#!/bin/bash
# Userdata script for mon2y-dist server

# Variables
SERVICE_USER="mon2y"
APP_DIR="/opt/mon2y/dist"
VENV_DIR="$APP_DIR/venv"
DB_DIR="/var/db/mon2y"
S3_BUCKET="mon2y"
# This should be replaced with the actual wheel filename or a script to find the latest
DIST_WHEEL="dist/mon2y_dist-0.1.0-py3-none-any.whl"
OPTUNA_STORAGE="sqlite:///$DB_DIR/db.sqlite3"

# Install dependencies
yum update -y
yum install -y python3.10 python3.10-pip python3.10-devel gcc aws-cli

# Create user and directories
useradd -r -m -d /home/$SERVICE_USER -s /bin/bash $SERVICE_USER
mkdir -p $APP_DIR
mkdir -p $DB_DIR
chown -R $SERVICE_USER:$SERVICE_USER /opt/mon2y
chown -R $SERVICE_USER:$SERVICE_USER $DB_DIR

# Set up virtual environment
python3.10 -m venv $VENV_DIR
chown -R $SERVICE_USER:$SERVICE_USER $VENV_DIR

# Install wheel
su - $SERVICE_USER -c "aws s3 cp s3://$S3_BUCKET/$DIST_WHEEL $APP_DIR/"
su - $SERVICE_USER -c "$VENV_DIR/bin/pip install $APP_DIR/$(basename $DIST_WHEEL)"

# Create systemd service file
cat > /etc/systemd/system/mon2y-dist.service <<EOF
[Unit]
Description=Mon2y Distributed Optuna Service
After=network.target

[Service]
User=$SERVICE_USER
Group=$SERVICE_USER
WorkingDirectory=$APP_DIR
Environment="OPTUNA_STORAGE=$OPTUNA_STORAGE"
Environment="S3_BUCKET=$S3_BUCKET"
ExecStart=$VENV_DIR/bin/gunicorn --workers 3 --bind 0.0.0.0:5000 mon2y_dist.main:app
Restart=always
SyslogIdentifier=mon2y-dist

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
systemctl daemon-reload
systemctl enable mon2y-dist.service
systemctl start mon2y-dist.service
