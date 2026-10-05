mk p2
printf 'l1\nl2\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "add f"
git checkout -q -b feat
printf 'l1\nfeat\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "feat edits f"
echo more > g.txt; git add g.txt; tick; git commit -qm "feat adds g"
git checkout -q main
printf 'l1\nmain\nl3\n' > f.txt; git add f.txt; tick; git commit -qm "main edits f"
ex2 git cherry-pick feat~1
git status
git status --short
cat f.txt
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO'
cat .git/CHERRY_PICK_HEAD
cat .git/MERGE_MSG
git ls-files -s f.txt
ex2 git commit -m resolved
ex2 git cherry-pick --continue
ex2 git cherry-pick --skip
ex2 git cherry-pick --abort
git status
git log --oneline
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO'
echo "--- again: resolve and continue"
ex2 git cherry-pick feat~1
printf 'l1\nresolved\nl3\n' > f.txt
git add f.txt
GIT_EDITOR=true ex2 git cherry-pick --continue
git log --format='%h %an %s%n%b' -2
git show --stat --format=%B HEAD | head
git reset -q --hard HEAD~1
echo "--- multi with conflict on first, continue"
ex2 git cherry-pick feat~1 feat
ls .git/sequencer
cat .git/sequencer/todo
cat .git/sequencer/head
cat .git/sequencer/opts 2>&1
git status | head -8
printf 'l1\nresolved\nl3\n' > f.txt
git add f.txt
GIT_EDITOR=true ex2 git cherry-pick --continue
git log --oneline
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO'
git reset -q --hard HEAD~2
echo "--- multi: abort"
git cherry-pick feat~1 feat >/dev/null 2>&1
ex2 git cherry-pick --abort
git status | head -3
git log --oneline | head -2
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO'
echo "--- multi: skip"
git cherry-pick feat~1 feat >/dev/null 2>&1
ex2 git cherry-pick --skip
git log --oneline | head -3
git status | head -3
git reset -q --hard main
echo "--- multi: quit"
git cherry-pick feat~1 feat >/dev/null 2>&1
ex2 git cherry-pick --quit
git status | head -3
ls .git | grep -iE 'pick|seq|ORIG|MERGE|AUTO'
git reset -q --hard main
