./target/release/utwid_auto \
  --exploration \
  --plain-mode \
  -T 4 \
  -s 4 \
  -S 8 \
  --iteration-set 2000, 5000, 10000 \
  --difficulty-mod-set 0.2,1,2 \
  --random \
  --exploration-constant-set .3 \
  --reward-level-set 20,100 \
  --reward-win-set 20,100 \
  --reward-lose-set=-5,-10\
  --reward-stalemate-set=-20 \
  --reward-health-bias-set=-0.3,0 \
  --reward-turn-weight-set=0.1 \
  --reward-passivity-penalty-set 0,1,2,5 \
  -vv

