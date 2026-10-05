R="$(cat /s/r.py)"
W="$(cat /s/w.py)"
echo "--- missing"; python3 -c "$R" nofile; echo "exit $?"
echo "--- noarg"; python3 -c "$R"; echo "exit $?"
echo "--- dir"; mkdir d; python3 -c "$R" d; echo "exit $?"
echo "--- badutf8"; printf 'a,b\n\xff\n' > bad.csv; python3 -c "$R" bad.csv; echo "exit $?"
echo "--- trunc utf8"; printf 'a,b\n\xe2\x82' > bad2.csv; python3 -c "$R" bad2.csv; echo "exit $?"
echo "--- cont"; printf 'a,\xc3\x28\n' > bad3.csv; python3 -c "$R" bad3.csv; echo "exit $?"
for off in 4000 4095 4096 5000 8191 8192 8193 10000 20000; do
python3 -c "import sys; sys.stdout.buffer.write(b'a'*$off + b'\xff\n')" > off.csv
echo "--- off $off"; python3 -c "$R" off.csv 2>&1 | grep Unicode
done
echo "--- bigfield"; python3 -c "print('a,'+'x'*131073)" > big.csv; python3 -c "$R" big.csv; echo "exit $?"
echo "--- bigfield ok"; python3 -c "print('a,'+'x'*131072)" > big2.csv; python3 -c "$R" big2.csv | wc -c
echo "--- bigfield quoted"; python3 -c "print('a,\"'+'x'*131073+'\"')" > big3.csv; python3 -c "$R" big3.csv; echo "exit $?"
echo "--- stdout closed";  python3 -c "$R" big2.csv >&-; echo "exit $?"
