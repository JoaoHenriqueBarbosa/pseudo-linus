P="aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr ssss"
run(){ printf '%s\n' "$2" > in.txt; echo "## $1"; ptx -w 72 in.txt | awk -v k="$3" '{ if (index(substr($0,40),k)==1) print }'; }
run "base" "$P tttt" tttt
run "long word at start" "XXXXXXXXXXXXXXXXXXXX $P tttt" tttt
run "long word after kw" "$P tttt YYYYYYYYYYYYYYYYYYYY" tttt
run "long word in head region" "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmmmmmmmmmmmmmmmmmm nnnn oooo pppp qqqq rrrr ssss tttt" tttt
printf '%s tttt.  Other sentence with a LONGWORDLONGWORDLONGWORD here.\n' "$P" > in.txt; echo "## second sentence long word"; ptx -w 72 in.txt | awk '{ if (index(substr($0,40),"tttt")==1) print }'
