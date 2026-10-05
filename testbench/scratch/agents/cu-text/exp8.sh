for L in 1 2 3 4 5 6 7 8; do
  words=""
  for i in $(seq 0 15); do
    c=$(printf "\\$(printf '%03o' $((97 + i)))")
    w=$(printf "%${L}s" "" | tr ' ' "$c")
    words="$words $w"
  done
  text="${words# } DDDDDDDD EEEEEEEEE FFFFFFFFFF zz"
  printf '%s\n' "$text" > in.txt
  echo "@@ L=$L"
  ptx -w 72 in.txt | grep -E " zz( |\$)" | grep -vE "zz [a-zA-Z]" | grep -E "^ +[A-Za-z]"
  ptx -w 72 in.txt | awk '{ if (substr($0,40,2)=="zz") print }'
done
