import csv, sys, json
for row in csv.reader(open(sys.argv[1], newline='', encoding='utf-8')):
    print(json.dumps(row, ensure_ascii=False))
