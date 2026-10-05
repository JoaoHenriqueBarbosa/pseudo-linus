mk p4
printf 'l1\nl2\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "add f"
printf 'l1\nl2 changed\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "change f"
printf 'l1\nl2 again\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "change f again"
ex2 git revert HEAD~1
cat f.txt
git status | head -12
cat .git/MERGE_MSG
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO|REVERT'
cat .git/REVERT_HEAD
printf 'l1\nl2 fixed\nl3\n' > f.txt; git add f.txt
ex2 git revert --continue
git reflog | head -3
git log -1 --format='%an %ae %s%n%b'
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO|REVERT'
echo "--- conflict then commit directly"
ex2 git revert HEAD~1
printf 'l1\nl2 fixed2\nl3\n' > f.txt; git add f.txt
ex2 git commit
EDITOR=true ex2 git commit
git reflog | head -2
echo "--- revert skip multi"
git reset -q --hard HEAD~1
ex2 git revert HEAD~1 HEAD
cat .git/sequencer/todo
cat .git/sequencer/head
ex2 git revert --skip
git status | head -4
git log --oneline | head -3
echo "--- pick: cherry-pick -x conflict + continue"
git reset -q --hard
git checkout -q -b side
printf 'l1\nside\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "side edit" -m "side body"
git checkout -q main
ex2 git cherry-pick -x side
printf 'l1\nresolved\nl3\n' > f.txt; git add f.txt
ex2 git cherry-pick --continue
git log -1 --format=%B
git reflog | head -2
echo "--- pick merge commit"
git checkout -q -b m1 main~2
echo m1 > m1.txt; git add m1.txt; tick; git commit -qm "m1"
git checkout -q main
git merge -q --no-ff m1 -m "merge m1"
ex2 git cherry-pick HEAD
ex2 git cherry-pick -m 1 HEAD
ex2 git cherry-pick -m 3 HEAD
ex2 git cherry-pick -m 0 HEAD
ex2 git revert HEAD
git status --short
ex2 git revert -m 1 HEAD
git log -1 --format=%B
echo "--- root commit"
ex2 git cherry-pick $(git rev-list --max-parents=0 HEAD)
ex2 git revert $(git rev-list --max-parents=0 HEAD)
