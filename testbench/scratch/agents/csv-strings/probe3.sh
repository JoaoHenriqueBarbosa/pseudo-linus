W="$(cat /s/w.py)"
t() { echo "--- $1"; printf "$1" | python3 -c "$W" 2>&1; echo "exit $?"; }
t '[1,]\n'
t '{"a":1,}\n'
t '[1 2]\n'
t '{"a" 1}\n'
t '{1:2}\n'
t '"abc\n'
t '"a\\qb"\n'
t '"a\\u12"\n'
t '"a\\u12zz"\n'
t '"a\x01b"\n'
t '[nul]\n'
t '\xef\xbb\xbf[1]\n'
t '["\\ud800"]\n'
t '["\\udcff"]\n'
t '[1e999, -1e999, NaN, Infinity, -Infinity]\n'
t '[01]\n'
t '[1.]\n'
t '[-]\n'
python3 -c "print('9'*5000)" > /tmp/big.json
echo "--- bigint"; python3 -c "$W" < /tmp/big.json 2>&1 | tail -5
echo "--- bigint in list"; python3 -c "print('[' + '9'*5000 + ']')" | python3 -c "$W" 2>&1 | tail -5
echo "--- rec"
for n in 900 980 985 988 990 992 994 996 1000; do
  printf "$n: "; python3 -c "print('['*$n + ']'*$n)" | python3 -c "$W" 2>&1 | tail -1
done
echo "--- rec obj"
for n in 980 990 995; do
  printf "$n: "; python3 -c "print('{\"a\":'*$n + '1' + '}'*$n)" | python3 -c "$W" 2>&1 | tail -1
done
