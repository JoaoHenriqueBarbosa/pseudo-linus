W="$(cat /s/w.py)"
for n in 9993 9995 9996 9997 9998 9999 10000 10002; do
  printf "$n: "; python3 -c "print('['*$n + ']'*$n)" | python3 -c "$W" 2>&1 | tail -c 40 | tr '\n' '~'; echo
done
for n in 9996 9998 9999 10000; do
  printf "obj $n: "; python3 -c "print('{\"a\":'*$n + '1' + '}'*$n)" | python3 -c "$W" 2>&1 | tail -c 40| tr '\n' '~'; echo
done
