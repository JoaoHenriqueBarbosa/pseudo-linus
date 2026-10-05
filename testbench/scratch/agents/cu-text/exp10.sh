letters=(a b c d e f g h i j k l m n o p q r s t u v w x y z)
run(){ printf '%s\n' "$2" > in.txt; ptx -w $1 in.txt | awk '{ if (substr($0,'"$((3+$1/2+1))"',2)=="zz") print }'; }
for W in 72 50; do
for n in 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
  Bm=$((W/2 - 5))
  L=$(( (Bm - (n-1)) / n ))
  [ $L -lt 2 ] && continue
  wd=""
  for i in $(seq 1 $n); do
    c=$(printf "\\$(printf '%03o' $((64 + i)))")
    w=$(printf "%${L}s" "" | tr ' ' "$c")
    wd="$wd $w"
  done
  text="${letters[*]:0:26}$wd zz"
  echo "## W=$W n=$n L=$L blen=$((n*L+n-1))"
  run $W "$text"
done
done
