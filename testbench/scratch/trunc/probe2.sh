cd /tmp && mkdir d && cd d && mkdir dir && printf 'hello world\n' > dir/a.txt && head -c 3000 /dev/zero | tr '\0' x > dir/b.txt && tar cf ../full.tar dir && cd ..
for n in 4396 4100 5000 1100; do
  echo "== $n"; head -c $n full.tar > t.tar; tar -tvf t.tar 2>&1; echo "rc=$?"
  echo "-- pipe"; cat t.tar | tar -tf - 2>&1; echo "rc=$?"
  echo "-- gz"; gzip -c t.tar | tar -tzf - 2>&1; echo "rc=$?"
  echo "-- xO"; tar -xOf t.tar 2>&1 | od -c | head -3; echo "rc=$?"
done
