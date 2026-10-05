import csv, sys, json
w = csv.writer(sys.stdout, lineterminator='\n')
for line in sys.stdin:
    w.writerow(json.loads(line))
