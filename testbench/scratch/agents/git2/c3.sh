mk r
git checkout -q -b other HEAD~1
echo other > b.txt; git add b.txt; echo o > o.txt; git add o.txt; tick; git commit -qm other
git checkout -q main
echo "== carry local mod on unrelated file"
echo mod > a.txt
ex2 git checkout other
git status --short
git checkout -q -f main
echo "== local mod on file differing between branches"
echo mod > b.txt
ex2 git checkout other
git status --short
git checkout -q -f main
echo "== staged mod on file differing"
echo mod > b.txt; git add b.txt
ex2 git checkout other
echo "== untracked would be overwritten"
git checkout -q -f main
git reset -q --hard
echo untracked > o.txt
ex2 git checkout other
cat o.txt
rm o.txt
echo "== staged new file carried"
echo n > n.txt; git add n.txt
ex2 git checkout other
git status --short
git reset -q --hard main 2>/dev/null; git checkout -q -f main
echo "== multiple files conflict"
echo mod > b.txt; echo mod > c.txt;
git checkout -q -b other2 HEAD~2
ex2 git checkout main
echo "== deleted file carried"
git checkout -q -f main
rm a.txt
ex2 git checkout other
git status --short
echo "== -m"
git checkout -q -f main
echo mod > b.txt
ex2 git checkout -m other
cat b.txt
git status --short
echo "== checkout -f b/ from local mod"
git checkout -q -f main
echo mod > b.txt
ex2 git checkout -f other
cat b.txt
git status --short
ex2 git checkout -b
ex2 git checkout -b x y z
ex2 git checkout -b x -B y
ex2 git checkout --detach -b x
ex2 git checkout -b x --orphan y
ex2 git checkout --foo
ex2 git switch --foo
