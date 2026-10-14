#!/bin/bash
set -euxo pipefail

HOME_DIR=/home/ec2-user
BIN_DIR="$HOME_DIR/bin"
WORK_DIR="$HOME_DIR/work"

dnf install -y tmux awscli curl

mkdir -p "$BIN_DIR" "$WORK_DIR"

# ----------------------------
# Write test runner (fully pinned paths)
# ----------------------------
cat <<'EOF' > /home/ec2-user/test.sh
#!/bin/bash
set -euxo pipefail

cd /home/ec2-user/work

/home/ec2-user/utwid_auto \
  --exploration \
  --plain-mode \
  -T 4 \
  -s 4 \
  -S 8 \
  --iteration-set 1000,2000,5000,10000 \
  --difficulty-mod-set 0.2,1,2 \
  --random \
  --exploration-constant-set .3,1.4 \
  --reward-level-set 5,20,100 \
  --reward-win-set 5,20,100 \
  --reward-lose-set=-5,-10,-20,-100 \
  --reward-stalemate-set=-5,-20,-20 \
  --reward-health-bias-set=-0.3,0 \
  --reward-turn-weight-set=0.1,0.2,0.5,1.0 \
  -vv
EOF

chmod +x /home/ec2-user/test.sh
chown ec2-user:ec2-user /home/ec2-user/test.sh

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

# ----------------------------
# Start tmux session
# ----------------------------
tmux new-session -d -s utwid -n main

# Pane 0: worker 1
tmux send-keys -t utwid:main.0 \
  "cd /home/ec2-user/work && /home/ec2-user/test.sh" C-m

# Split right: worker 2
tmux split-window -h -t utwid:main.0
tmux send-keys -t utwid:main.1 \
  "cd /home/ec2-user/work && /home/ec2-user/test.sh" C-m

# Split bottom: uploader
tmux split-window -v -t utwid:main.0

tmux send-keys -t utwid:main.2 "
INSTANCE_ID=$INSTANCE_ID
while true; do
  aws s3 cp /home/ec2-user/explore.json \
    s3://utwid/result/explore-\${INSTANCE_ID}.json
  sleep 60
done
" C-m

# Optional layout cleanup
tmux select-layout -t utwid:main tiled
'
