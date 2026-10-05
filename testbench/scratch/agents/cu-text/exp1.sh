printf 'aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr ssss tttt\n' > in.txt
for w in 30 40 50 60 72 80 90 100 120; do
  echo "== w=$w"
  ptx -w $w in.txt | grep -E '  tttt($| )'
done
