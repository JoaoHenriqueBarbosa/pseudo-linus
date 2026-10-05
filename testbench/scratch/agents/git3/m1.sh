mk m1
git branch side
echo d > d.txt; git add d.txt; tick; git commit -qm "add d"
git checkout -q side
echo s > s.txt; git add s.txt; tick; git commit -qm "add s"
git checkout -q main
echo "--- ff-able: create ffb from main"
git branch ffb
git checkout -q ffb
echo f > f.txt; git add f.txt; tick; git commit -qm "add f"
git checkout -q main
ex2 git merge ffb
git log --oneline --graph
git reset -q --hard HEAD~1
ex2 git merge --ff-only ffb
git reset -q --hard HEAD~1
ex2 git merge --no-ff ffb
git log --oneline --graph
cat .git/ORIG_HEAD
git reset -q --hard HEAD~1
ex2 git merge --no-ff -m "custom msg" ffb
git log -1 --format=%B
git reset -q --hard HEAD~1
ex2 git merge --squash ffb
git status --short
cat .git/SQUASH_MSG
ls .git | grep -i MERGE
git reset -q --hard
ex2 git merge --no-commit --no-ff ffb
git status
ls .git | grep -i MERGE
cat .git/MERGE_HEAD .git/MERGE_MODE .git/MERGE_MSG
ex2 git commit -m done
git log --oneline --graph
git reset -q --hard HEAD~1
echo "--- non-ff clean"
ex2 git merge side
git log --oneline --graph
git cat-file -p HEAD
git reflog | head -5
ex2 git merge side
ex2 git merge ffb
ex2 git merge nope
ex2 git merge
ex2 git merge ffb side
git reset -q --hard HEAD~1
ex2 git merge side ffb
ex2 git merge --ff-only side
ex2 git merge -m "x" --ff-only side
git log --oneline --graph
git reset -q --hard main~1
ex2 git merge --ff-only side
git log --oneline --graph --all
