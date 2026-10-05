mk p3
printf 'l1\nl2\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "add f"
printf 'l1\nl2 changed\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "change f" -m "details"
echo extra > g.txt; git add g.txt; tick; git commit -qm "add g"
ex2 git revert HEAD
git log -1 --format=%B
git log --format='%h %an %s' -3
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO|REVERT'
git reflog | head -2
echo "--- revert the revert"
ex2 git revert HEAD
git log -1 --format=%B
echo "--- -n"
ex2 git revert -n HEAD~2
git status --short
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO|REVERT'
cat .git/MERGE_MSG
git reset -q --hard
echo "--- revert with conflict"
printf 'l1\nl2 other\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "other edit"
ex2 git revert HEAD~3
git status --short
cat f.txt
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO|REVERT'
cat .git/MERGE_MSG
git status | head -8
GIT_EDITOR=true ex2 git revert --continue
ex2 git revert --skip
git status | head -3
ex2 git revert --abort
echo "--- revert multiple"
git reset -q --hard HEAD
git log --oneline | head -3
ex2 git revert HEAD HEAD~1
git log --format='%s' -3
echo "--- revert -x / -m / edit / signoff / --no-commit multiple"
ex2 git revert --no-edit -s HEAD
git log -1 --format=%B
ex2 git revert -x HEAD
ex2 git revert
ex2 git revert nope
ex2 git revert --continue
ex2 git revert --skip
ex2 git revert --quit
ex2 git revert -e HEAD
ex2 git revert --reference HEAD
git log -1 --format=%B
