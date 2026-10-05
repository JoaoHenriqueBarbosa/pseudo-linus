mk m7
# base: a,b,c (each "x\n")
git branch other
echo "--- modify/delete"
git checkout -q other
git rm -q b.txt; tick; git commit -qm "other deletes b"
git checkout -q main
echo changed > b.txt; git add b.txt; tick; git commit -qm "main modifies b"
ex2 git merge other
git status --short
git ls-files -s b.txt
cat b.txt
git merge --abort
echo "--- reverse: main deletes, other modifies"
git reset -q --hard HEAD~1
git checkout -q other
git reset -q --hard HEAD~1
echo changed > b.txt; git add b.txt; tick; git commit -qm "other modifies b"
git checkout -q main
git rm -q b.txt; tick; git commit -qm "main deletes b"
ex2 git merge other
git status --short
git ls-files -s b.txt
git merge --abort
echo "--- add/add differing"
git reset -q --hard HEAD~1
git checkout -q other; git reset -q --hard HEAD~1
echo A > new.txt; git add new.txt; tick; git commit -qm "other adds new"
git checkout -q main; git reset -q --hard HEAD~1
echo B > new.txt; git add new.txt; tick; git commit -qm "main adds new"
ex2 git merge other
git status --short
cat new.txt
git ls-files -s new.txt
git merge --abort
echo "--- add/add identical"
git checkout -q other; git reset -q --hard HEAD~1
echo B > new.txt; git add new.txt; tick; git commit -qm "other adds new same"
git checkout -q main
ex2 git merge other
git log --oneline --graph | head -3
git reset -q --hard HEAD~1
echo "--- rename in other, modify in main"
git checkout -q other; git reset -q --hard HEAD~1
git mv c.txt c2.txt; tick; git commit -qm "other renames c"
git checkout -q main; git reset -q --hard HEAD~1
echo more >> c.txt; git add c.txt; tick; git commit -qm "main edits c"
ex2 git merge other
cat c2.txt
git status --short
git log -1 --stat --format=%s
git reset -q --hard HEAD~1
echo "--- rename vs delete"
git checkout -q other; git reset -q --hard HEAD~1
git mv c.txt c2.txt; tick; git commit -qm "other renames c"
git checkout -q main; git reset -q --hard HEAD~1
git rm -q c.txt; tick; git commit -qm "main removes c"
ex2 git merge other
git status --short
git ls-files -s
git merge --abort
echo "--- rename/rename 1to2"
git reset -q --hard HEAD~1
git checkout -q main
git mv c.txt c3.txt; tick; git commit -qm "main renames c to c3"
ex2 git merge other
git status --short
git ls-files -s
git merge --abort
echo "--- both rename same"
git reset -q --hard HEAD~1
git mv c.txt c2.txt; tick; git commit -qm "main renames c to c2"
ex2 git merge other
git log --oneline --graph | head -3
git reset -q --hard HEAD~1
