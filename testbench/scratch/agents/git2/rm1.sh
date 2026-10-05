mk r
mkdir -p d/e; echo 1 > d/e/f.txt; echo 2 > d/g.txt; git add d; tick; git commit -qm d
echo "== rm basic"
ex2 git rm a.txt
git status --short
ls
ex2 git rm a.txt
ex2 git rm nonexist
ex2 git rm -q b.txt
git status --short
ex2 git rm --cached c.txt
git status --short
ls
ex2 git rm c.txt
ex2 git rm -f c.txt
git status --short
git reset -q --hard
echo "== rm modified"
echo mod > a.txt
ex2 git rm a.txt
ex2 git rm -f a.txt
git reset -q --hard
echo mod > a.txt; git add a.txt
ex2 git rm a.txt
ex2 git rm --cached a.txt
git status --short
git reset -q --hard
echo mod > a.txt; git add a.txt; echo mod2 > a.txt
ex2 git rm a.txt
ex2 git rm --cached a.txt
git reset -q --hard
echo mod > a.txt; git add a.txt; git commit -qm x; echo mod2 > a.txt;
ex2 git rm --cached a.txt
git reset -q --hard
echo "== rm -r dirs"
ex2 git rm d
ex2 git rm -r d
ls
git reset -q --hard
ex2 git rm -rq d
git reset -q --hard
ex2 git rm -n a.txt
ls a.txt
ex2 git rm -r -n d
ex2 git rm 'd/*'
git reset -q --hard
ex2 git rm -r --cached d
ls d
git reset -q --hard
ex2 git rm --ignore-unmatch nonexist
ex2 git rm -r nonexist
ex2 git rm a.txt a.txt
git reset -q --hard
ex2 git rm a.txt nonexist
git status --short
git reset -q --hard
echo "== untracked"
echo u > u.txt
ex2 git rm u.txt
ex2 git rm -f u.txt
echo "== subdir"
cd d
ex2 git rm g.txt
ex2 git rm ../a.txt
cd ..
git reset -q --hard
ls
echo "== rm of dir with modified file"
echo mod >> d/g.txt
ex2 git rm -r d
echo mod >> a.txt
echo mod >> b.txt
ex2 git rm a.txt b.txt
git reset -q --hard
echo "== rm after file already deleted"
rm a.txt
ex2 git rm a.txt
git status --short
git reset -q --hard
rm a.txt
ex2 git rm --cached a.txt
ex2 git rm -r d --dry-run
ex2 git rm -r -f d -q
ls
