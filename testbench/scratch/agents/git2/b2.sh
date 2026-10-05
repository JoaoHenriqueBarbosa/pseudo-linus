mk r
git branch feature HEAD~1
ex git branch -m feature feat2
cat .git/logs/refs/heads/feat2; ls .git/logs/refs/heads
ex git branch -m main trunk
cat .git/HEAD
cat .git/logs/HEAD | cut -c83-
cat .git/logs/refs/heads/trunk | cut -c83-
ex git branch -M trunk feat2
git branch
cat .git/logs/HEAD | cut -c83-
ex git branch -m trunk
ex git branch -m
ex git branch -m a b c
ex git branch -c
ex git branch -C feat2 trunk
ex git branch -c nonexist foo
ex git branch -m nonexist foo
ex git branch -d
ex git branch --delete
ex git branch -d trunk
ex git branch trunk2 trunk
ex git branch trunk3 nonexistent
ex git branch trunk4 HEAD~1 extra
ex git branch -f trunk2 HEAD~1
ex git branch -f trunk
ex git branch -f trunk3 HEAD
git branch -v
ex git branch --show-current
git checkout -q --detach
ex git branch --show-current
ex git branch
ex git branch -v
ex git branch -vv
ex git branch --list
ex git branch -d trunk2
ex git branch --merged
ex git branch --no-merged
ex git branch --contains trunk2
