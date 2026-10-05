P="aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr ssss tttt"
run(){ printf '%s\n' "$2" > in.txt; echo "## $1"; ptx -w 72 in.txt | awk '{ if (substr($0,40,4)=="aaaa") print }'; }
run "base" "$P"
run "long word at end" "$P XXXXXXXXXXXXXXXXXXXX"
run "long word at start?" "XXXXXXXXXXXXXXXXXXXX $P"
printf '%s.  Other sentence LONGWORDLONGWORDLONGWORD here.\n' "$P" > in.txt; echo "## second sentence long word"; ptx -w 72 in.txt | awk '{ if (substr($0,40,4)=="aaaa") print }'
printf '%s\n' "$P" > in.txt;  printf 'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZ.\n' > in2.txt; echo "## other FILE long word"; ptx -w 72 in.txt in2.txt | awk '{ if (substr($0,40,4)=="aaaa") print }'
echo "## ignore the long word"; printf 'XXXXXXXXXXXXXXXXXXXX\n' > ign; printf 'XXXXXXXXXXXXXXXXXXXX %s tttt\n' "$P" > in.txt; ptx -i ign -w 72 in.txt | awk '{ if (index(substr($0,40),"tttt")==1) print }'
echo "## only mode"; printf 'tttt\n' > only; ptx -o only -w 72 in.txt | awk '{ if (index(substr($0,40),"tttt")==1) print }'
echo "## digits long"; printf '12345678901234567890 %s tttt\n' "$P" > in.txt; ptx -w 72 in.txt | awk '{ if (index(substr($0,40),"tttt")==1) print }'
echo "## underscore long"; printf 'xxxx_xxxx_xxxx_xxxx_xxxx %s tttt\n' "$P" > in.txt; ptx -w 72 in.txt | awk '{ if (index(substr($0,40),"tttt")==1) print }'
