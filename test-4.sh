./target/release/utwid_auto \
  --exploration \
  --plain-mode \
  -T 2 \
  -s 2 \
  -S 8 \
  --iteration-set 1000,2000,5000,10000 \
  --difficulty-mod-set 0.2,1,2 \
  --random \
  --exploration-constant-set .1,.3,1,1.4,2 \
  --reward-level-set 5,10,20,100 \
  --reward-win-set 5,10,20,100 \
  --reward-lose-set=-5,-10,-20,-100 \
  --reward-stalemate-set=-5,-20,-20 \
  --reward-health-bias-set=-0.3,0 \
  --reward-turn-weight-set=0.1,0.2,0.5,1.0 \
  -vv

