text="a b c d e f g h i j k l m n o p q r s t u v w x y z AAAAA BBBBBB CCCCCCC DDDDDDDD EEEEEEEEE FFFFFFFFFF zz"
printf '%s\n' "$text" > in.txt
for g in 3 1; do
for W in $(seq 40 1 120); do
  echo "@@ g=$g W=$W"
  ptx -w $W -g $g in.txt
done
done
