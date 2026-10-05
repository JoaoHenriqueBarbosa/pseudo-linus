mk r
git checkout -q -b topic; echo t > t.txt; git add t.txt; tick; git commit -qm topic1
git checkout -q main
ex git branch -d topic
ex git branch -D topic
git branch topic3 HEAD
git config branch.topic3.description hello
git config branch.topic3.remote .
git config branch.topic3.merge refs/heads/main
cat .git/config
ex git branch -d topic3
cat .git/config
ls .git/logs/refs/heads
git checkout -q -b topic4; echo t > t.txt; git add t.txt; tick; git commit -qm topic4
git config branch.topic4.remote .
git config branch.topic4.merge refs/heads/main
git checkout -q main
ex git branch -d topic4
ex git branch -d -q topic4
ex git branch -dq topic4
ex git branch -D -q topic4
ex git branch -d -f topic4
ex git branch -f -d topic4
echo --- merged into upstream but not HEAD
git checkout -q -b up1; echo u > u.txt; git add u.txt; tick; git commit -qm up1
git checkout -q main
git branch up2 up1
git config branch.up2.remote .
git config branch.up2.merge refs/heads/up1
ex git branch -d up2
git update-ref refs/heads/gone HEAD~2
git config branch.gone.remote .
git config branch.gone.merge refs/heads/up1
ex git branch -d gone
git config branch.gone.merge refs/heads/zzzz
ex git branch -d gone
echo --- tag nonbranch
ex git branch -d main
echo --- ref with slash
git branch feat/x HEAD~1
ls .git/refs/heads
ex git branch -d feat/x
ls .git/refs/heads
echo --- dir/file conflict
git branch feat/y
ex git branch feat
ex git branch feat/y/z
ex git branch -m feat/y feat
ex git branch -m feat/y feat/z
git branch
echo --- invalid names
ex git branch -- -x
ex git branch HEAD
ex git branch ''
ex git branch 'a b'
ex git branch 'a:b'
ex git branch @
ex git branch 'x.lock'
ex git branch '.x'
ex git branch 'x/'
ex git branch main
ex git branch -d HEAD
