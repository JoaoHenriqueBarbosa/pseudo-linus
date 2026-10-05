mk m4
git branch side
echo d > d.txt; git add d.txt; tick; git commit -qm "add d"
git checkout -q side
echo s > s.txt; echo "s2" >> a.txt; git add s.txt a.txt; tick; git commit -qm "side changes"
git checkout -q main
echo "--- staged unrelated"
echo new > n.txt; git add n.txt
ex2 git merge side
git status --short
git reset -q --hard
echo "--- staged unrelated two files"
echo new > n.txt; echo new > o.txt; git add n.txt o.txt
ex2 git merge side
git reset -q --hard
echo "--- worktree dirty overlapping"
echo dirty >> a.txt
ex2 git merge side
git status --short
git checkout -q a.txt
echo "--- untracked overwritten"
echo mine > s.txt
ex2 git merge side
git status --short
ls .git | grep -E 'MERGE|AUTO'
rm s.txt
echo "--- ff dirty overlapping"
git reset -q --hard
git branch ffx side
git reset -q --hard main
git checkout -q -b ffy main
git reset -q --hard main
git checkout -q main
git reset -q --hard HEAD~1
echo dirty >> a.txt
ex2 git merge side
git status --short
git checkout -q a.txt
echo mine > s.txt
ex2 git merge side
git status --short
rm s.txt
echo "--- ff with staged"
echo new > n.txt; git add n.txt
ex2 git merge side
git status --short
git log --oneline --graph
