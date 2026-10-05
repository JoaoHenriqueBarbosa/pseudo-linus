letters=(a b c d e f g h i j k l m n o p q r s t u v w x y z A B C D E F G H I J K L M N O P Q R S T U V W X Y Z)
for N in 12 14 16 18 20 22 26 30 36 44 52; do
  text="${letters[*]:0:$N}"
  printf '%s\n' "$text" > in.txt
  for W in $(seq 30 2 100); do
    echo "@@ N=$N W=$W"
    ptx -w $W in.txt
  done
done
