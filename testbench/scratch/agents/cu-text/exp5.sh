printf 'a b c d e f g h i j k l m n o p q r s t u v w x y z A B C D E F G H I J K L M N O P Q R S T U V W X Y Z 0 1 2 3 4 5 6 7 8 9\n' > in.txt
for w in 40 50 60 72 90 110; do for g in 3; do
echo "== w=$w g=$g"
ptx -w $w -g $g in.txt | tail -3
done; done
