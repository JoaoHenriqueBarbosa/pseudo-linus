mk m2
printf 'l1\nl2\nl3\nl4\nl5\n' > f.txt; git add f.txt; tick; git commit -qm "f"
git checkout -q -b other
printf 'l1\nl2 other\nl3\nl4\nl5 other\n' > f.txt; echo extra > g.txt; git add f.txt g.txt; tick; git commit -qm "other changes"
git checkout -q main
printf 'l1\nl2 main\nl3\nl4\nl5\n' > f.txt; git add f.txt; tick; git commit -qm "main changes"
ex2 git merge other
git status
git status --short
git ls-files -s
cat f.txt
ls .git | grep -i -E 'MERGE|ORIG'
cat .git/MERGE_MSG
cat .git/MERGE_MODE | od -c | head
git diff
git diff --cached
ex2 git commit -m x
ex2 git merge --continue
ex2 git merge other
ex2 git merge --abort
git status
git status --short
cat f.txt
ls .git | grep -i -E 'MERGE|ORIG|AUTO'
ex2 git merge --abort
ex2 git merge --continue
ex2 git merge --quit
echo "--- again, resolve"
git merge other >/dev/null 2>&1
printf 'l1\nresolved\nl3\nl4\nl5 other\n' > f.txt
git add f.txt
git status
ex2 git merge --continue
git log --oneline --graph
git show --stat --format=%B HEAD
git cat-file -p HEAD | head -8
