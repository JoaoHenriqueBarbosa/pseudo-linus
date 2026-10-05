mk r
git branch feature HEAD~1
ex git checkout feature
ex git checkout feature
ex git checkout main
ex git checkout -
ex git checkout -
ex git switch -
ex git checkout -b new
ex git checkout -b new
ex git checkout -B new
ex git checkout -B new2 HEAD~1
ex git checkout -b
ex git checkout nonexist
ex git checkout -q feature
ex git checkout -b n3 -q
ex git checkout --detach
ex git checkout --detach main
ex git checkout HEAD~1
ex git checkout main
ex git checkout -d feature
ex git checkout $(git rev-parse main)
ex git checkout main~1
cat .git/logs/HEAD | cut -c83-
git checkout -q main
ex git checkout HEAD
ex git checkout -q HEAD
ex git checkout main --
ex git checkout -- a.txt
ex git checkout HEAD -- a.txt
ex git checkout HEAD~1 -- c.txt
ex git status --short
ex git checkout -- nonexist
ex git checkout HEAD -- nonexist
ex git checkout .
echo changed > a.txt
ex git checkout a.txt
cat a.txt
echo changed > a.txt
git add a.txt
echo changed2 > a.txt
ex git checkout -- a.txt
cat a.txt
git status --short
ex git checkout -f main
ex git checkout --orphan orph
git status | head -3
