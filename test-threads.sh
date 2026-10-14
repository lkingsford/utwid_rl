#!/bin/bash
for i in {1..16}; do \
./utwid_auto \
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
  -vv &> "log_$i.log" & done;
