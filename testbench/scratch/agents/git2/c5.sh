mk r
git remote add origin /nonexistent
git update-ref refs/remotes/origin/dev HEAD~1
echo "== -b tracking streams"
ex2 git checkout -b t5 origin/dev
ex2 git checkout main
ex2 git switch -c t6 origin/dev
ex2 git switch t5
ex2 git checkout t5
ex2 git checkout --track origin/dev
ex2 git checkout --track -b zz origin/dev
echo "== guess"
git update-ref refs/remotes/origin/guessme HEAD~1
ex2 git checkout guessme
ex2 git branch -vv
cat .git/config | tail -4
git checkout -q main
ex2 git switch guessme2
git update-ref refs/remotes/origin/g2 HEAD
ex2 git switch g2
git checkout -q main
ex2 git checkout --no-guess g2
ex2 git checkout -
echo "== orphan"
ex2 git checkout --orphan newroot
git status --short
git log --oneline 2>&1 | head -2
ex2 git checkout main
echo "== unborn"
rm -rf u; git init -q -b main u; cd u
ex2 git checkout -b first
ex2 git checkout main
ex2 git switch -c second
ex2 git checkout --detach
ex2 git branch
ex2 git reset
ex2 git reset --hard
ex2 git reset --soft
ex2 git tag t1
ex2 git branch x
ex2 git checkout x
ex2 git switch x
echo hi > f; git add f
ex2 git reset
ex2 git reset f
git status --short
ex2 git rm --cached f
ex2 git rm -f f
cd ..
echo "== unmerged"
mk m
git checkout -q -b side; echo s > a.txt; git commit -qam side; git checkout -q main; echo m > a.txt; git commit -qam mainchange
ex2 git merge side
git status --short
ex2 git checkout side
ex2 git switch side
ex2 git checkout -f side
git status --short
