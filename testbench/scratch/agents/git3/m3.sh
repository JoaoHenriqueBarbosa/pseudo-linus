mk m3
git branch side
echo d > d.txt; git add d.txt; tick; git commit -qm "add d"
git checkout -q side
echo s > s.txt; git add s.txt; tick; git commit -qm "add s"
git checkout -q main
ex2 git merge --ff-only side
ex2 git merge --squash --no-ff side
ex2 git merge --squash --commit side
ex2 git merge --no-ff --ff-only side
ex2 git merge -h
ex2 git merge --abort extra
ex2 git merge --continue extra
ex2 git merge --quit x
git checkout -q -b up
ex2 git merge
git config branch.up.remote origin
ex2 git merge
git config branch.up.merge refs/heads/zzz
ex2 git merge
git checkout -q main
git checkout -q --detach
ex2 git merge
git checkout -q main
echo "--- unrelated"
git checkout -q --orphan orphan
git rm -rfq .
echo o > o.txt; git add o.txt; tick; git commit -qm orphan
git checkout -q main
ex2 git merge orphan
ex2 git merge --allow-unrelated-histories orphan
git log --oneline --graph
git reset -q --hard HEAD~1
echo "--- dirty"
echo dirty >> d.txt
ex2 git merge side
git checkout -q d.txt
echo "--- untracked would be overwritten"
echo mine > s.txt
ex2 git merge side
ex2 git merge --no-ff side
rm s.txt
echo "--- staged change not conflicting"
echo new > n.txt; git add n.txt
ex2 git merge side
git status --short
git reset -q --hard
echo "--- merge msg from tag, commit, remote, HEAD~1"
git tag v1 side
ex2 git merge --no-ff v1
git log -1 --format=%s
git reset -q --hard HEAD~1
git tag -a -m annotated v2 side
ex2 git merge v2
git log -1 --format=%B
git reset -q --hard HEAD~1
ex2 git merge $(git rev-parse side)
git log -1 --format=%s
git reset -q --hard HEAD~1
ex2 git merge side~0
git log -1 --format=%s
git reset -q --hard HEAD~1
git commit -q --allow-empty -m e; git branch s2 side^0
git checkout -q -b topic main~1
echo t > t.txt; git add t.txt; tick; git commit -qm topic
ex2 git merge side
git log -1 --format=%s
git reset -q --hard HEAD~1
ex2 git merge main
git log -1 --format=%s
git reset -q --hard HEAD~1
ex2 git merge main side
git log -1 --format=%s
git reset -q --hard HEAD~1
ex2 git merge --into-name foo side
git log -1 --format=%s
git reset -q --hard HEAD~1
ex2 git merge --log side
git log -1 --format=%B
git reset -q --hard HEAD~1
ex2 git merge --no-stat side
ex2 git merge -n side
git reset -q --hard HEAD~1
ex2 git merge -q side
git reset -q --hard HEAD~1
ex2 git merge -e -m "edit msg" side
git reset -q --hard HEAD~1
ex2 git merge --no-edit -m "noedit" side
git log -1 --format=%B
git reset -q --hard HEAD~1
ex2 git merge --signoff side
git log -1 --format=%B
git reset -q --hard HEAD~1
ex2 git merge -F /dev/null side
ex2 git merge -m "" side
git log -1 --format=%B
