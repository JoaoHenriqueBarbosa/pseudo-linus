mk r
echo "== checkout -b with local mods"
echo mod > a.txt
ex2 git checkout -b nb
ex2 git checkout --detach HEAD~1
git checkout -q -f main
echo "== checkout <tree> -- path"
git branch old HEAD~1
echo X > b.txt
ex2 git checkout old -- b.txt
git status --short
ex2 git checkout HEAD~2 -- b.txt c.txt
ex2 git checkout old -- .
ex2 git checkout old
ex2 git checkout old b.txt
ex2 git checkout HEAD~1 b.txt
ex2 git checkout -q HEAD~1 b.txt
echo "== path with ambiguity"
git branch b.txt
ex2 git checkout b.txt
ex2 git checkout -- b.txt
ex2 git checkout b.txt --
echo "== -b with -- path"
echo "== path in subdir"
mkdir -p d/e; echo 1 > d/e/f.txt; git add d; tick; git commit -qm d
rm -rf d
ex2 git checkout d
ls d/e
rm -rf d
ex2 git checkout -- d/e/f.txt
cd d 2>/dev/null && { ex2 git checkout -- e/f.txt; ex2 git checkout . ; cd ..; }
echo "== restore"
echo mod > a.txt
ex2 git restore a.txt
cat a.txt
echo mod > a.txt; git add a.txt
ex2 git restore a.txt
cat a.txt
ex2 git restore --staged a.txt
git status --short
ex2 git restore --staged --worktree a.txt
git status --short
echo mod > a.txt; git add a.txt
ex2 git restore --source=HEAD~2 --staged --worktree a.txt
ex2 git restore -s HEAD~1 a.txt
ex2 git restore --source HEAD~1 nonexist
ex2 git restore nonexist
ex2 git restore -S -W .
ex2 git restore .
git status --short
ex2 git restore --source=nonexist a.txt
ex2 git restore -q a.txt
ex2 git restore --staged
ex2 git restore --ours a.txt
ex2 git restore -s HEAD~1
