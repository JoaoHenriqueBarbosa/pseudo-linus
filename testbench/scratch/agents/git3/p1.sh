mk p1
git checkout -q -b feat
echo f1 > f1.txt; git add f1.txt; tick; git commit -qm "feat one"
echo f2 > f2.txt; git add f2.txt; tick; git commit -qm "feat two" -m "body line"
git checkout -q main
ex2 git cherry-pick feat~1
git log --format='%h %an %ad %s' -2
git status --short
git reflog | head -3
ls .git | grep -iE 'pick|seq|ORIG|MERGE'
echo "--- -x"
git reset -q --hard HEAD~1
ex2 git cherry-pick -x feat
git log -1 --format=%B
git reset -q --hard HEAD~1
echo "--- -n"
ex2 git cherry-pick -n feat~1
git status --short
ls .git | grep -iE 'pick|seq|ORIG|MERGE'
git reset -q --hard
echo "--- range"
ex2 git cherry-pick main..feat
git log --oneline | head -4
ls .git | grep -iE 'pick|seq|ORIG|MERGE'
git reset -q --hard main~0
git reset -q --hard HEAD~2
echo "--- edit/signoff"
ex2 git cherry-pick -s feat~1
git log -1 --format=%B
git reset -q --hard HEAD~1
echo "--- errors"
ex2 git cherry-pick
ex2 git cherry-pick nope
ex2 git cherry-pick feat~1 nope
ex2 git cherry-pick -m 1 feat~1
ex2 git cherry-pick --continue
ex2 git cherry-pick --abort
ex2 git cherry-pick --skip
ex2 git cherry-pick --quit
ex2 git cherry-pick HEAD
git status --short
echo "--- already applied (empty)"
git cherry-pick feat~1 >/dev/null 2>&1
ex2 git cherry-pick feat~1
git status --short
ex2 git cherry-pick --allow-empty feat~1
ex2 git cherry-pick --keep-redundant-commits feat~1
git log --oneline | head -5
