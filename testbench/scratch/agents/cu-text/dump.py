import glob,json,sys,tomllib
root='/home/john/projects/pseudo-linus/.claude/worktrees/cu-text/testbench/'
cases={}
for f in glob.glob(root+'corpus/cases/coreutils/*.toml'):
    d=tomllib.load(open(f,'rb'))
    for c in d['case']:
        cases[c['id']]=c
gold={}
for f in glob.glob(root+'golden/coreutils/*.json'):
    gold.update(json.load(open(f)))
for n in sys.argv[1:]:
    c=cases[n]; g=gold[n]
    print('=====',n)
    print('argv',c['argv'], {k:v for k,v in c.items() if k not in('id','argv','files','tags')})
    for k,v in c.get('files',{}).items(): print('  file',k,repr(v))
    print('  stdout',repr(g['stdout'])); print('  stderr',repr(g['stderr'])); print('  exit',g['exit'])
