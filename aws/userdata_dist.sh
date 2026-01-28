#!/bin/bash
set -euo pipefail
# Userdata script for mon2y-dist server

### REQUIRED EARLY VAR
PG_VERSION=17

# Variables
SERVICE_USER="optuna"
APP_DIR="/opt/mon2y/dist"
VENV_DIR="$APP_DIR/venv"
DB_DIR="/var/db/mon2y"
S3_BUCKET="mon2y"
DIST_WHEEL="mon2y/mon2y_dist-0.1.0-py3-none-any.whl"
OPTUNA_STORAGE="postgresql+psycopg2://optuna@/optuna"
PGDATA="/var/lib/pgsql/${PG_VERSION}/data"
MOUNT_POINT="/var/lib/pgsql"
DB_NAME="optuna"
DB_USER="optuna"

### BASIC SETUP
dnf update -y
dnf install -y \
  postgresql${PG_VERSION}-server \
  postgresql${PG_VERSION} \
  util-linux \
  python3.13 \
  python3.13-pip \
  awscli \
  xfsprogs

### USERS + DIRS
id "$SERVICE_USER" &>/dev/null || useradd -r -m -d /home/$SERVICE_USER -s /bin/bash $SERVICE_USER
mkdir -p "$APP_DIR" "$DB_DIR"
chown -R $SERVICE_USER:$SERVICE_USER /opt/mon2y "$DB_DIR"

### PYTHON
python3.13 -m venv "$VENV_DIR"
chown -R $SERVICE_USER:$SERVICE_USER "$VENV_DIR"

su - $SERVICE_USER -c "aws s3 cp s3://$S3_BUCKET/$DIST_WHEEL $APP_DIR/"
su - $SERVICE_USER -c "$VENV_DIR/bin/pip install $APP_DIR/$(basename $DIST_WHEEL)"

### SYSTEMD SERVICE
cat > /etc/systemd/system/mon2y-dist.service <<EOF
[Unit]
Description=Mon2y Distributed Optuna Service
After=network.target postgresql-${PG_VERSION}.service
Requires=postgresql-${PG_VERSION}.service

[Service]
User=$SERVICE_USER
Group=$SERVICE_USER
WorkingDirectory=$APP_DIR
Environment="OPTUNA_STORAGE=$OPTUNA_STORAGE"
Environment="S3_BUCKET=$S3_BUCKET"
ExecStart=$VENV_DIR/bin/gunicorn --workers 3 --bind 0.0.0.0:5000 mon2y_dist.main:app
Restart=always

[Install]
WantedBy=multi-user.target
EOF


#
VOLUME_TAG_NAME="Mon2y DB"
IMDS="http://169.254.169.254/latest"
TOKEN=$(curl -s -X PUT "$IMDS/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600")

INSTANCE_ID=$(curl -s -H "X-aws-ec2-metadata-token: $TOKEN" \
  "$IMDS/meta-data/instance-id")

AZ=$(curl -s -H "X-aws-ec2-metadata-token: $TOKEN" \
  "$IMDS/meta-data/placement/availability-zone")

REGION="${AZ::-1}"
## ---------------- EBS ATTACH ----------------

VOLUME_ID=$(aws ec2 describe-volumes \
  --region "$REGION" \
  --filters Name=tag:Name,Values="$VOLUME_TAG_NAME" Name=availability-zone,Values="$AZ" \
  --query 'Volumes[0].VolumeId' \
  --output text)

[ "$VOLUME_ID" = "None" ] && { echo "EBS volume not found"; exit 1; }

aws ec2 attach-volume \
  --region "$REGION" \
  --volume-id "$VOLUME_ID" \
  --instance-id "$INSTANCE_ID" \
  --device /dev/sdf

### WAIT FOR NVME DEVICE
for i in {1..30}; do
  REAL_DEVICE=$(lsblk -o NAME,SERIAL | \
    awk "/$(echo $VOLUME_ID | sed 's/-//')/ {print \"/dev/\" \$1}")
  [ -n "$REAL_DEVICE" ] && break
  sleep 2
done

[ -z "$REAL_DEVICE" ] && { echo "EBS device not visible"; exit 1; }

### FILESYSTEM
mkdir -p "$MOUNT_POINT"

if ! blkid "$REAL_DEVICE"; then
  mkfs.xfs "$REAL_DEVICE"
fi

mountpoint -q "$MOUNT_POINT" || mount "$REAL_DEVICE" "$MOUNT_POINT"

grep -q "$REAL_DEVICE" /etc/fstab || \
  echo "$REAL_DEVICE $MOUNT_POINT xfs defaults,nofail 0 2" >> /etc/fstab

### POSTGRES PERMS BEFORE INIT
mkdir -p "$PGDATA"
chown -R postgres:postgres "$MOUNT_POINT"
chmod 700 "$MOUNT_POINT"

### INITDB (IDEMPOTENT)
if [ ! -f "$PGDATA/PG_VERSION" ]; then
  sudo -u postgres /usr/bin/postgresql--setup initdb
fi

### POSTGRES CONFIG
CONF="$PGDATA/postgresql.conf"
HBA="$PGDATA/pg_hba.conf"

sed -i "s/^#listen_addresses.*/listen_addresses = 'localhost'/" "$CONF"

cat >> "$CONF" <<EOF
shared_buffers = 128MB
work_mem = 4MB
maintenance_work_mem = 64MB
max_connections = 20
EOF

cat > "$HBA" <<EOF
local   all             postgres                                peer
local   all             ${DB_USER}                               peer
local   all             ${SERVICE_USER}                          peer
local   all             all                                     peer
host    all             all             127.0.0.1/32            reject
host    all             all             ::1/128                 reject
EOF

systemctl enable postgresql-${PG_VERSION}
systemctl start postgresql-${PG_VERSION}

### DATABASE (IDEMPOTENT)
sudo -u postgres psql <<EOF
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '${DB_USER}') THEN
    CREATE ROLE ${DB_USER} LOGIN;
  END IF;
END
\$\$;

DO \$\$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_database WHERE datname = '${DB_NAME}') THEN
    CREATE DATABASE ${DB_NAME} OWNER ${DB_USER};
  END IF;
END
\$\$;
EOF

### START APP
systemctl daemon-reload
systemctl enable mon2y-dist.service
systemctl start mon2y-dist.service
