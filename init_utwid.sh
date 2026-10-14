#!/bin/bash
set -euxo pipefail

HOME_DIR=/home/ec2-user
BIN_DIR="$HOME_DIR/bin"
WORK_DIR="$HOME_DIR/work"

dnf install -y tmux

mkdir -p "$BIN_DIR" "$WORK_DIR"

# ----------------------------
# Switch to ec2-user context
# ----------------------------
su - ec2-user -c '
set -euxo pipefail

HOME_DIR=/home/ec2-user
WORK_DIR="$HOME_DIR/work"

mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

INSTANCE_ID=$(curl -s http://169.254.169.254/latest/meta-data/instance-id)

# ----------------------------
# Fetch latest binary (aarch64)
# ----------------------------
LATEST=$(aws s3 ls s3://utwid/binaries/ \
  | grep aarch64 \
  | sort \
  | tail -n1 \
  | awk "{print \$4}")

aws s3 cp "s3://utwid/binaries/$LATEST" "$HOME_DIR/"
tar -xf "$HOME_DIR/$LATEST" -C "$HOME_DIR"

chmod +x "$HOME_DIR/utwid_auto" || true

# ----
# Push results
# ----
#
# Get IMDSv2 token
TOKEN=$(curl -s -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600")
# Get instance ID using token
INSTANCE_ID=$(curl -s -H "X-aws-ec2-metadata-token: $TOKEN" http://169.254.169.254/latest/meta-data/instance-id)

if [ -z "$INSTANCE_ID" ]; then
  echo "ERROR: Failed to get instance ID from metadata service" >&2
  exit 1
fi
echo "Instance ID: $INSTANCE_ID"

while true; do
  # Use || true to continue even if aws command fails
  aws s3 cp /home/ec2-user/explore.json \
    "s3://utwid/result/explore-${INSTANCE_ID}.json" || true
  sleep 60
done &

# ---
# Start tests
# ---

for i in {1..16}; do \
./target/release/utwid_auto \
  --exploration \
  --plain-mode \
  --threads-set 1,2,4 \
  --simulation-threads-set 1,2,8 \
  --simulations-set 1,2,8 \
  --iteration-set 2000 \
  --difficulty-mod-set 1 \
  --random \
  --exploration-constant-set .3 \
  --reward-level-set 20 \
  --reward-win-set 20 \
  --reward-lose-set=-10 \
  --reward-stalemate-set=-20 \
  --reward-health-bias-set=-0.3 \
  --reward-turn-weight-set 0.1 \
  --reward-passivity-penalty-set 2.0 \
  --reward-health-weight-set 2.5 \
  --level-base-set 0.75 \
  -vv &> "log_{i}.log" & done &
'
