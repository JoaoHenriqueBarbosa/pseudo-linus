mk r
echo mod > a.txt; echo new > n.txt; git add n.txt; echo b2 >> b.txt
git rm -q --cached c.txt
echo "== reset mixed"
ex2 git reset
git status --short
echo "== reset soft"
git add -A; git status --short
ex2 git reset --soft HEAD~1
git status --short
cat .git/logs/HEAD | cut -c83-
cat .git/ORIG_HEAD
echo "== reset hard"
ex2 git reset --hard
git status --short
ex2 git reset --hard HEAD~1
ex2 git reset --hard main
ex2 git reset --hard HEAD~2
ls
git reflog
cat .git/logs/refs/heads/main | cut -c83-
echo "== mixed to older commit"
git reset -q --hard ORIG_HEAD
ex2 git reset HEAD~1
git status --short
ex2 git reset HEAD~1 -- a.txt
ex2 git reset -- a.txt
ex2 git reset a.txt
ex2 git reset HEAD a.txt
ex2 git reset nonexist
ex2 git reset HEAD nonexist
ex2 git reset -- nonexist
git reset -q --hard main
echo "== reset paths with changes"
echo mod > a.txt; git add a.txt; echo mod2 > b.txt; git add b.txt
ex2 git reset a.txt
git status --short
ex2 git reset
ex2 git reset -q
ex2 git reset --mixed
ex2 git reset --mixed -q HEAD
ex2 git reset --soft -- a.txt
ex2 git reset --hard -- a.txt
ex2 git reset --soft --hard
ex2 git reset --merge
ex2 git reset --keep HEAD~1
git status --short
git reset -q --hard main
ex2 git reset nonexistentrev
ex2 git reset --hard nonexistentrev
ex2 git reset HEAD~10
echo "== reset on detached/new branch"
git checkout -q --detach
ex2 git reset --hard HEAD~1
git checkout -q main
echo "== untracked file"
echo u > u.txt
ex2 git reset --hard
ls
echo "== reset with new file in older commit"
git reset -q --hard main
echo "== unstaged after reset with rename"
echo dd > d.txt; git add d.txt; tick; git commit -qm d
echo mod >> d.txt
ex2 git reset HEAD~1
git status --short
