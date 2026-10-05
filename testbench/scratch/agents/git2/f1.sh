mk r
git remote add origin /nonexistent
git update-ref refs/remotes/origin/main HEAD~1
git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
git config branch.main.remote origin
git config branch.main.merge refs/heads/main
git tag v1 HEAD~1
git tag -a -m "annotated one" v2 HEAD~2
git branch other HEAD~2
tick
ex2 git for-each-ref
ex2 git for-each-ref refs/heads
ex2 git for-each-ref refs/heads/
ex2 git for-each-ref refs/heads/m*
ex2 git for-each-ref 'refs/tags/v*'
ex2 git for-each-ref heads
ex2 git for-each-ref --count=2
ex2 git for-each-ref --count=0
ex2 git for-each-ref --sort=-refname
ex2 git for-each-ref --sort=objecttype --sort=refname
ex2 git for-each-ref --format='%(refname) %(objecttype) %(objectsize)'
ex2 git for-each-ref --format='%(refname:short)|%(refname:lstrip=1)|%(refname:lstrip=2)|%(refname:rstrip=1)|%(refname:strip=2)|%(refname:rstrip=-1)'
ex2 git for-each-ref --format='%(objectname) %(objectname:short) %(objectname:short=10) %(objectname:short=3)'
ex2 git for-each-ref --format='%(tree) %(parent) %(numparent)' refs/heads
ex2 git for-each-ref --format='%(author) | %(authorname) | %(authoremail) | %(authordate) | %(committerdate:iso) | %(committerdate:short) | %(authoremail:trim) | %(authoremail:localpart)' refs/heads/main
ex2 git for-each-ref --format='%(creator) | %(creatordate) | %(creatordate:unix)' refs/heads/main refs/tags
ex2 git for-each-ref --format='%(subject)|%(body)|%(contents)|%(contents:subject)|%(contents:body)|%(contents:signature)|%(contents:lines=1)' refs/heads/main refs/tags/v2
ex2 git for-each-ref --format='%(tag) %(taggername) %(taggeremail) %(taggerdate) %(object) %(type)' refs/tags
ex2 git for-each-ref --format='%(*objectname) %(*objecttype) %(*subject) %(*authorname)' refs/tags
ex2 git for-each-ref --format='%(upstream) %(upstream:short) %(upstream:track) %(upstream:trackshort) %(upstream:remotename) %(upstream:remoteref) %(upstream:track,nobracket)' refs/heads
ex2 git for-each-ref --format='%(push) %(push:short) %(push:track)' refs/heads
ex2 git for-each-ref --format='%(HEAD) %(refname:short)'
ex2 git for-each-ref --format='%(symref) %(symref:short) %(refname)' refs/remotes
ex2 git for-each-ref --format='%(worktreepath)' refs/heads
ex2 git for-each-ref --format='%(refname:short)%(if)%(upstream)%(then) T%(else) F%(end)' refs/heads
ex2 git for-each-ref --format='%(if:equals=main)%(refname:short)%(then)is main%(else)not%(end)' refs/heads
ex2 git for-each-ref --format='%(align:10)%(refname:short)%(end)|' refs/heads
ex2 git for-each-ref --format='%(align:10,right)%(refname:short)%(end)|' refs/heads
ex2 git for-each-ref --format='%(color:red)x%(color:reset)' refs/heads/main
ex2 git for-each-ref --format='%%x%xx41' refs/heads/main
ex2 git for-each-ref --format='%(foo)' refs/heads/main
ex2 git for-each-ref --format='%(refname' refs/heads/main
ex2 git for-each-ref --format='%(refname:foo)' refs/heads/main
ex2 git for-each-ref --shell --format='%(refname) %(subject)' refs/heads/main
ex2 git for-each-ref --perl --format='%(refname) %(subject)' refs/heads/main
ex2 git for-each-ref --python --format='%(refname) %(subject)' refs/heads/main
ex2 git for-each-ref --tcl --format='%(refname) %(subject)' refs/heads/main
ex2 git for-each-ref --points-at=HEAD~2
ex2 git for-each-ref --contains HEAD~1 refs/heads
ex2 git for-each-ref --no-contains HEAD~1 refs/heads
ex2 git for-each-ref --merged HEAD~1
ex2 git for-each-ref --no-merged HEAD~1
ex2 git for-each-ref --exclude='refs/tags/*'
ex2 git for-each-ref --omit-empty --format='%(upstream)' refs/heads
ex2 git for-each-ref --include-root-refs
ex2 git for-each-ref --stdin </dev/null
ex2 git for-each-ref --ignore-case 'REFS/HEADS/*'
