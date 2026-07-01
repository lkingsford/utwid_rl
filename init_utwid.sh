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

chmod +x "$HOME_DIR/utwid_auto" || true'
