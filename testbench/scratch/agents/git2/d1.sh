mk r
git tag v1 HEAD~1
git update-ref refs/remotes/origin/x HEAD~2
git branch other HEAD~1
for t in v1 origin/x HEAD~1 HEAD~2 $(git rev-parse HEAD) $(git rev-parse --short HEAD~1) other main; do
  git checkout -q --detach $t 2>&1
  echo "--- $t"; git status | head -1; git branch | head -1
done
echo "== commit moved"
git checkout -q --detach v1
echo x > x.txt; git add x.txt; tick; git commit -qm x
git status | head -1; git branch | head -1
git checkout -q --detach main
git branch | head -1
git checkout -q main
git checkout -q v1 2>&1
git branch --show-current
git branch -vv | head -3
git branch --list | head -2
git checkout -q main
git checkout -q HEAD
git branch | head -1
