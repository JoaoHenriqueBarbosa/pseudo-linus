mk r4
ex2 git remote add -f origin /nonexistent
cat .git/config
ex2 git remote add -f o2 /work
ex2 git remote add -f o3 file:///nonexistent
ex2 git remote add -f o4 relative/path
ex2 git remote add -f -m main o5 /nonexistent
cat .git/config
ls .git/refs/remotes
ex2 git remote update
ex2 git remote update o2
ex2 git remote update -p
ex2 git remote show o3
ex2 git remote show o4
ex2 git remote -v show o4
ex2 git remote prune o4
ex2 git remote set-head o4 -a
ex2 git remote set-head o4 -d
ex2 git remote set-head o4 main
git update-ref refs/remotes/o4/main HEAD
git update-ref refs/remotes/o4/dev HEAD
ex2 git remote set-head o4 main
cat .git/refs/remotes/o4/HEAD
ex2 git remote set-head o4 -a main
ex2 git remote set-head o4 -d main
ex2 git remote set-head o4 nope
ex2 git remote set-head o4 -d
ex2 git remote set-head o4 -d
ls .git/refs/remotes/o4
ex2 git branch -r
ex2 git remote rename o4 o9
ex2 git branch -r
ex2 git remote set-branches o9 a b
cat .git/config
ex2 git remote set-branches --add o9 c
ex2 git remote set-branches o9 z
cat .git/config
ex2 git remote rename o9 o9
ex2 git remote rename o9 origin2
ex2 git remote rename o9 origin2
ex2 git remote rename origin2 o2
ex2 git remote rename origin2 'bad name'
cat .git/config
git update-ref refs/remotes/o9/main HEAD
ex2 git remote rm o9
git config remote.fx.url /fx
git config remote.fx.fetch '+refs/heads/main:refs/remotes/fx/m'
git update-ref refs/remotes/fx/m HEAD
git update-ref refs/remotes/fx/other HEAD
git branch --track mine fx/m
git config branch.mine.pushremote fx
git branch -a
cat .git/config
ex2 git remote rm fx
git branch -a
cat .git/config
ex2 git remote
