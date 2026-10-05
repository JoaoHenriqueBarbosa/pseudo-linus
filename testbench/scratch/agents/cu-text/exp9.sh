run(){ printf '%s\n' "$2" > in.txt; echo "## $1: $2"; ptx -w 72 in.txt | awk -v k="$3" '{ if (index(substr($0,40),k)==1) print }'; }
P="aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm"
run "T1 tttt" "$P nnnn oooo pppp qqqq rrrr ssss tttt" tttt
run "T1 kw zz" "$P nnnn oooo pppp qqqq rrrr ssss zz" zz
run "before 3 words of 9" "$P xxxxxxxxx yyyyyyyyy wwwwwwwww zz" zz
run "before 5 words" "$P nnnn oooo pppp qqqq rrrrrrrrrr zz" zz
run "before 4 words 7s" "$P nnnnnnn ooooooo ppppppp qqqqqqq zz" zz
run "before 2 words" "$P nnnnnnnnnnnnnn oooooooooooooo zz" zz
run "before 6 words 3s" "$P nnn ooo ppp qqq rrr sss ttt uuu zz" zz
