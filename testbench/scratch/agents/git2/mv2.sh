mk r
mkdir -p d/e; echo 1 > d/e/f.txt; echo 2 > d/g.txt; git add d; tick; git commit -qm d
mkdir nd tgt
echo "== into itself"
ex2 git mv d d/sub
ex2 git mv -k d d/sub
ex2 git mv d/e d/e/x
echo "== dir to existing dir"
ex2 git mv d nd
git status --short
ls nd
git reset -q --hard; git checkout -q -f main 2>/dev/null; rm -rf nd; mkdir nd; git status --short
ls
echo "== to dir with slash"
ex2 git mv a.txt nd/
ex2 git mv b.txt nd/b2.txt
ex2 git mv c.txt tgt
git status --short
echo "== multiple to dir"
git reset -q --hard
git mv -k a.txt b.txt nd >/dev/null 2>&1
git status --short
git reset -q --hard
ex2 git mv a.txt b.txt nd
git status --short
git reset -q --hard
echo "== multiple to nonexistent dir"
ex2 git mv a.txt b.txt nope
echo "== -k with errors in multiple"
ex2 git mv -k a.txt nonexist b.txt nd
git status --short
git reset -q --hard
echo "== untracked source"
echo u > u.txt
ex2 git mv u.txt nd
ex2 git mv -k u.txt nd
echo "== overwrite dir with file"
ex2 git mv a.txt d
ex2 git mv -f a.txt d
ex2 git mv d/g.txt a.txt
ex2 git mv -f d/g.txt a.txt
git status --short
git reset -q --hard
echo "== rename in subdir"
cd d
ex2 git mv g.txt h.txt
ex2 git mv ../a.txt .
ex2 git mv e ../nd
ex2 git mv h.txt ../tgt/
cd ..
git status --short
echo "== mv verbose multi"
git reset -q --hard
ex2 git mv -v a.txt b.txt nd
echo "== dry run"
git reset -q --hard
ex2 git mv -n a.txt zzz
ls a.txt
ex2 git mv -n a.txt b.txt nd
ex2 git mv -nv a.txt zzz
echo "== sparse"
ex2 git mv --sparse a.txt zz
git reset -q --hard
echo "== source with trailing slash"
ex2 git mv d/ dd
git status --short
git reset -q --hard
echo "== source dir exists untracked, dest missing dir parent"
ex2 git mv a.txt nonexist/sub/a.txt
echo "== mv to same file in other form"
ex2 git mv a.txt ./a.txt
ex2 git mv a.txt nd/../a.txt
echo "== mv file onto dir with same name"
mkdir a2; echo 3 > a2/x
git add a2
ex2 git mv a.txt a2
