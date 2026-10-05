git init -q -b main r && cd r
echo a > a.txt; git add a.txt; tick; git commit -qm one
echo b > b.txt; git add b.txt; tick; git commit -qm two
ex git branch
ex git branch -v
ex git branch feature
ex git branch -v
ex git branch -vv
ex git branch --list
ex git branch --list 'f*'
ex git branch --show-current
ex git branch -a
ex git branch -r
ex git branch feature
ex git branch -d feature
ex git branch -d feature
ex git branch -D nonexist
ex git branch bad..name
ex git branch -m feature
ex git branch -m main main2
ex git branch -m main2 main
ex git branch x HEAD~1
ex git branch -c x y
ex git branch --contains HEAD~1
ex git branch --merged
ex git branch --no-merged
ex git branch --contains
ex git branch -d x
cat .git/logs/HEAD
echo ---
ls .git/logs/refs/heads
cat .git/logs/refs/heads/y
cat .git/config
git checkout -q -b new; echo c > c.txt; git add c.txt; tick; git commit -qm three
git checkout -q main
ex git branch -d new
ex git branch -D new
ex git branch -d y main
ex git branch -d main
ex git branch
