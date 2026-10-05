mk r
mkdir -p d/e; echo 1 > d/e/f.txt; echo 2 > d/g.txt; git add d; tick; git commit -qm d
mkdir empty
echo "== mv basic"
ex2 git mv a.txt z.txt
git status --short
ex2 git mv -v b.txt y.txt
ex2 git mv nonexist foo
ex2 git mv z.txt c.txt
ex2 git mv -f z.txt c.txt
git status --short
ex2 git mv -n y.txt x.txt
ls
ex2 git mv -k nonexist foo
ex2 git mv -k nonexist y.txt
ex2 git mv y.txt
ex2 git mv y.txt y.txt
ex2 git mv y.txt d
git status --short
ex2 git mv d d2
git status --short
ex2 git mv d2 d3
ex2 git mv d2/ d3
ex2 git mv d3 d2
ex2 git mv d2 empty
ex2 git mv d2/g.txt d2/e/f.txt .
ex2 git mv d2/g.txt .
ex2 git mv d2/e/f.txt nonexistdir/
ex2 git mv d2/e/f.txt nonexistdir/f.txt
ex2 git mv g.txt nonexistdir/
mkdir nd
ex2 git mv g.txt nd/
ex2 git mv -v g.txt nd/
git status --short
echo untracked > u.txt
ex2 git mv u.txt nd/
ex2 git mv c.txt u.txt
echo "== mv over existing untracked"
echo q > q.txt
ex2 git mv c.txt q.txt
ex2 git mv -f c.txt q.txt
git status --short
echo "== mv multiple to dir"
mkdir tgt
ex2 git mv nd/g.txt y.txt tgt
git status --short
ex2 git mv tgt/g.txt tgt/y.txt nonexistdir
ex2 git mv tgt nd
git status --short
echo "== mv in subdir"
cd nd
ex2 git mv tgt ../tgt2
cd ..
git status --short
echo "== mv modified file"
echo mod >> tgt2/g.txt
ex2 git mv tgt2/g.txt g2.txt
git status --short
echo "== mv symlink and exec"
ln -s a l1; git add l1
ex2 git mv l1 l2
echo "== mv into itself"
ex2 git mv d2 d2/sub
ex2 git mv -k d2 d2/sub
echo "== mv case dir"
ex2 git mv nd/ nd2
git status --short
ex2 git mv . x
ex2 git mv  'nd2/*' tgt2
