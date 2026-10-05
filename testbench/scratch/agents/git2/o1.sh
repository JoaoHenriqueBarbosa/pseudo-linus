mk r
git checkout -q --detach
echo x > x.txt; git add x.txt; tick; git commit -qm "lost one"
echo y > y.txt; git add y.txt; tick; git commit -qm "lost two"
ex2 git checkout main
cat .git/logs/HEAD | cut -c83- | tail -3
git checkout -q --detach HEAD
echo z > z.txt; git add z.txt; tick; git commit -qm "lost three"
ex2 git switch main
git checkout -q --detach
git commit -q --allow-empty -m one
git commit -q --allow-empty -m two
git branch keep
git commit -q --allow-empty -m three
ex2 git checkout main
git config advice.detachedHead false
git checkout -q --detach
git commit -q --allow-empty -m four
ex2 git checkout main
git checkout -q --detach
git commit -q --allow-empty -m five
ex2 git checkout -q main
ex2 git checkout --detach HEAD
ex2 git checkout --detach
