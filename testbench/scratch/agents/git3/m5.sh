mk m5
git branch side
echo d > d.txt; git add d.txt; tick; git commit -qm "add d"
git checkout -q side
echo s > s.txt; git add s.txt; tick; git commit -qm "side changes"
git checkout -q main
ls .git | grep -E 'AUTO|MERGE'
echo "-- after staged fail"
echo new > n.txt; git add n.txt
git merge side >/dev/null 2>&1
ls .git | grep -E 'AUTO|MERGE'
git reset -q --hard
echo "-- after untracked fail"
echo mine > s.txt
git merge side >/dev/null 2>&1
ls .git | grep -E 'AUTO|MERGE'
rm s.txt
echo "-- clean merge"
git merge side >/dev/null 2>&1
ls .git | grep -E 'AUTO|MERGE'
cat .git/AUTO_MERGE
git cat-file -t $(cat .git/AUTO_MERGE)
git log -1 --format=%T
git reset -q --hard HEAD~1
ls .git | grep -E 'AUTO|MERGE'
git merge --no-ff --no-commit side >/dev/null 2>&1
ls .git | grep -E 'AUTO|MERGE'
git commit -qm x
ls .git | grep -E 'AUTO|MERGE'
git reflog show AUTO_MERGE 2>&1 | head -3
ls .git/logs
