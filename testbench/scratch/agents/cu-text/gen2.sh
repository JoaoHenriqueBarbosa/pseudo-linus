mk(){ # len count
  python3 - "$1" "$2" <<'PY' 2>/dev/null || true
PY
}
for L in 1 2 3 4 5 6 8 10; do
  words=""
  for i in $(seq 0 29); do
    c=$(printf "\\$(printf '%03o' $((97 + i % 26)))")
    w=$(printf "%${L}s" "" | tr ' ' "$c")
    words="$words $w"
  done
  words="${words# }"
  printf '%s\n' "$words" > in.txt
  for w in 30 40 50 60 72 90; do
    for g in 1 3 5; do
      echo "@@ w=$w g=$g f=[/] t=[$words] opt=none"
      ptx -w $w -g $g in.txt
    done
  done
done
