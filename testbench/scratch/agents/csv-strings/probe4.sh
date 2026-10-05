W="$(cat /s/w.py)"
for n in 3000 5000 9000 9990 10010 12000; do
  printf "$n: "; python3 -c "print('['*$n + ']'*$n)" | python3 -c "$W" 2>&1 | tail -c 150 | tr '\n' '~'; echo
done
echo obj
for n in 4000 9000 12000; do
  printf "$n: "; python3 -c "print('{\"a\":'*$n + '1' + '}'*$n)" | python3 -c "$W" 2>&1 | tail -c 250| tr '\n' '~'; echo
done
