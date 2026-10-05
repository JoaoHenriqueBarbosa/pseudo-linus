R='import csv, sys, json
for row in csv.reader(open(sys.argv[1], newline='"'"'"'"'"', encoding='"'"'utf-8'"'"')):
    print(json.dumps(row, ensure_ascii=False))
'
W='import csv, sys, json
w = csv.writer(sys.stdout)
for line in sys.stdin:
    w.writerow(json.loads(line))
'
echo "--- missing"; python3 -c "$R" nofile; echo "exit $?"
echo "--- noarg"; python3 -c "$R"; echo "exit $?"
echo "--- dir"; mkdir d; python3 -c "$R" d; echo "exit $?"
echo "--- badutf8"; printf 'a,b\n\xff\n' > bad.csv; python3 -c "$R" bad.csv; echo "exit $?"
echo "--- trunc utf8"; printf 'a,b\n\xe2\x82' > bad2.csv; python3 -c "$R" bad2.csv; echo "exit $?"
echo "--- bigfield"; python3 -c "print('a,'+'x'*131073)" > big.csv; python3 -c "$R" big.csv | head -c 100; echo "exit $?"
echo "--- bigfield ok"; python3 -c "print('a,'+'x'*131072)" > big2.csv; python3 -c "$R" big2.csv | wc -c
echo "--- w blank"; printf '\n' | python3 -c "$W"; echo "exit $?"
echo "--- w extra"; printf '[1] x\n' | python3 -c "$W"; echo "exit $?"
echo "--- w int"; printf '5\n' | python3 -c "$W"; echo "exit $?"
echo "--- w types"; printf '[1, 2.5, null, true, [1,"a"], {"k":"v"}, 1e22, -0.0, 12345678901234567890]\n"abc"\n{"x":1,"y":2}\n' | python3 -c "$W"; echo "exit $?"
echo "--- w null"; printf 'null\n' | python3 -c "$W"; echo "exit $?"
echo "--- w badutf8"; printf '["a"]\n\xff\n' | python3 -c "$W"; echo "exit $?"
