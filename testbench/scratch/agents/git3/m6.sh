mk m6
git branch side
echo d > d.txt; git add d.txt; tick; git commit -qm "add d"
git checkout -q side
echo s > s.txt; echo x >> a.txt; git add s.txt a.txt; tick; git commit -qm "side changes"
git checkout -q main
chk() { echo "== $1: $(ls .git | grep -E 'AUTO|MERGE' | tr '\n' ' ')"; }
chk start
echo new > n.txt; git add n.txt
git merge side >/dev/null 2>&1
chk "staged fail"
git reset -q --hard
chk "after reset"
echo mine > s.txt
git merge side >/dev/null 2>&1
chk "untracked fail"
rm s.txt
echo dirty >> a.txt
git merge side >/dev/null 2>&1
chk "dirty fail"
git checkout -q a.txt
git merge --no-commit side >/dev/null 2>&1
chk "no-commit"
git merge --abort
chk "abort"
git merge --no-commit side >/dev/null 2>&1
git reset -q
chk "reset mixed"
git reset -q --hard
chk "reset hard"
git merge --no-commit side >/dev/null 2>&1
git merge --quit
chk "quit"
git reset -q --hard
git merge --no-commit side >/dev/null 2>&1
git commit -qm done
chk "commit"
git status --short
