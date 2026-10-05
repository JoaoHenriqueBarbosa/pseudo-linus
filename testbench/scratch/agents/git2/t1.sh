mk r
ex2 git tag
ex2 git tag v1
ex2 git tag
ex2 git tag v1
ex2 git tag -f v1 HEAD~1
ex2 git tag -f v1b HEAD~1
ex2 git tag v2 HEAD~2
ex2 git tag -l
ex2 git tag --list 'v*'
ex2 git tag -l 'v[12]'
ex2 git tag -l v1 v2
ex2 git tag -d v1b
ex2 git tag -d v1b
ex2 git tag -d v1 v2 nonexist
ex2 git tag -a va -m "annotated msg"
ex2 git tag -a vb -m "line1" -m "line2"
ex2 git tag -m "implies annotate" vc
ex2 git tag -a vd
ex2 git tag -a
ex2 git tag -n
ex2 git tag -n1
ex2 git tag -n3 -l 'v*'
ex2 git tag -l -n2
cat .git/refs/tags/va
git cat-file -p va
git cat-file -t va
git cat-file -p vb
git cat-file -p vc
git show-ref --tags
ex2 git tag -a vf -F -  </dev/null
printf 'from file\n\nbody\n' > msg.txt
ex2 git tag -a ve -F msg.txt
git cat-file -p ve
ex2 git tag --contains HEAD~1
ex2 git tag --contains HEAD
ex2 git tag --merged HEAD
ex2 git tag --no-merged HEAD
ex2 git tag --points-at HEAD
ex2 git tag --sort=-refname
ex2 git tag --sort=creatordate
ex2 git tag --format='%(refname:short) %(objecttype) %(subject)'
ex2 git tag 'bad name'
ex2 git tag bad..name
ex2 git tag HEAD
ex2 git tag -a -f va -m "forced"
ex2 git tag -a va -m "again"
ex2 git tag v9 nonexist
ex2 git tag v10 HEAD extra
ex2 git tag -d
ex2 git tag --delete va
ex2 git tag -l -d
ex2 git tag -a -m '' empty
ex2 git tag -a -m '   ' blank
ex2 git tag -a -m 'msg' dup HEAD~1
ex2 git tag -v dup
ex2 git tag -s foo -m x
ls .git/refs/tags
cat .git/logs/refs/tags/v1 2>&1 | head -2
ex2 git tag --create-reflog rl
ls .git/logs/refs/tags
git tag -a tt -m "tag of tag" va
git cat-file -p tt
