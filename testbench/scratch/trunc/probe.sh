cd /tmp && mkdir d && cd d && mkdir dir && printf 'hello world\n' > dir/a.txt && head -c 3000 /dev/zero | tr '\0' x > dir/b.txt && tar cf ../full.tar dir && cd ..
for n in 1024 1030 1036 1500 1536 1600 2048 2100 2560 3000 4096 5000; do
  rm -rf x && mkdir x && head -c $n full.tar > x/t.tar && cd x
  echo "== $n"; tar -xf t.tar 2>&1; echo "rc=$?"; find dir -type f -printf '%p %m %s\n' 2>/dev/null
  echo "-- tvf"; tar -tf t.tar 2>&1; echo "rc=$?"
  cd ..
done
