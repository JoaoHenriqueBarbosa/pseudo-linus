mk up
git branch dev
git branch feat/x
git tag v1
git tag -a -m ann v2
cd /work
git init -q -b main me
cd me
echo m > m.txt; git add m.txt; tick; git commit -qm m
ex2 git remote add -f origin /work/up
ex2 git remote add --no-tags origin2 /work/up
ex2 git remote add -m main orig3 /work/up
cat .git/config
ls .git/refs/remotes 2>&1
cat .git/refs/remotes/orig3/HEAD
ex2 git remote show origin
ex2 git remote show -n origin
ex2 git remote set-head origin -a
ex2 git remote update
ex2 git remote prune origin
ex2 git remote prune -n origin
git branch -q --track mine HEAD
git config branch.mine.remote origin
git config branch.mine.merge refs/heads/main
ex2 git remote show -n origin
ex2 git remote show origin
ex2 git remote rename origin up
cat .git/config
ex2 git remote rm up
cat .git/config
ex2 git remote -v
