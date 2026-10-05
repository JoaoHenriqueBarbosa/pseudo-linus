mk r
git tag v1 HEAD~1
ex git checkout HEAD~1
ex git checkout main
ex git checkout v1
ex git checkout main
ex git checkout -q v1
ex git checkout main
ex git switch --detach HEAD~2
ex git switch main
ex git switch -d v1
ex git switch main
ex git switch v1
ex git switch HEAD~1
ex git switch nonexist
ex git switch
ex git switch -c
ex git switch -c x -c y
ex git switch main extra
ex git switch -C x
ex git switch -C x
ex git switch -C y HEAD~1
ex git switch -c x
ex git switch -c z HEAD~1
ex git switch --orphan o1
ex git status | head -3
ex git switch main
ex git switch -
ex git switch -q -
ex git switch --guess nonexist
echo === advice off
git config advice.detachedHead false
ex git checkout HEAD~1
git checkout -q main
echo === checkout -b with detached
git checkout -q HEAD~1
ex git checkout -b fromdet
echo === checkout of commit with files updated
git checkout -q main
git checkout -q HEAD~2
ls
git checkout -q main
ls
echo === -f
echo dirty > a.txt
ex git checkout -f HEAD~2
cat a.txt
ex git status --short
