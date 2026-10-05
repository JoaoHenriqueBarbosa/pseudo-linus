mk r5
git update-ref refs/remotes/o/main HEAD
git remote add o /x
ex2 git remote set-head o main
find .git/logs -type f | sort
cat .git/logs/refs/remotes/o/HEAD
git remote add -m dev o2 /x
find .git/logs -type f | sort
ls -la .git/refs/remotes/o2/
cat .git/refs/remotes/o2/HEAD
git remote set-head o -d
find .git/logs -type f | sort
git branch -r
git update-ref refs/remotes/o/dev HEAD
git remote set-head o dev
git remote set-head o main
cat .git/logs/refs/remotes/o/HEAD
git branch -r
git remote rename o oo
find .git/logs -type f | sort
cat .git/logs/refs/remotes/oo/HEAD
cat .git/logs/refs/remotes/oo/main
git remote rm oo
find .git -path '*remotes*'
git config --get-regexp remote
git remote add pr /pr
git config remote.pr.push 'refs/heads/main:refs/heads/mainx'
git config --add remote.pr.push '+refs/heads/a*:refs/heads/b*'
git config --add remote.pr.push ':'
git config --add remote.pr.push ':refs/heads/del'
git config remote.pr.pushurl /ppp
git config --add remote.pr.pushurl /ppp2
ex2 git remote show -n pr
ex2 git remote -v
git config remote.pr.mirror true
ex2 git remote show -n pr
git remote add pr2 /pr2
git config --add remote.pr2.url /pr2b
ex2 git remote -v
ex2 git remote show -n pr2
git config remote.pr2.fetch 'refs/heads/*:refs/remotes/pr2/*'
git config --add remote.pr2.fetch 'refs/tags/*:refs/tags/*'
git update-ref refs/remotes/pr2/x HEAD
git update-ref refs/remotes/pr2/y HEAD
git symbolic-ref refs/remotes/pr2/HEAD refs/remotes/pr2/x
ex2 git remote show -n pr2
git config branch.main.remote pr2
git config branch.main.merge refs/heads/x
git config --add branch.main.merge refs/heads/y
ex2 git remote show -n pr2
git config branch.main.rebase true
ex2 git remote show -n pr2
git config branch.main.rebase interactive
ex2 git remote show -n pr2
git branch other
git config branch.other.remote pr2
git config branch.other.merge refs/heads/y
ex2 git remote show -n pr2
git config branch.main.rebase false
ex2 git remote show -n pr2
ex2 git remote show -n pr pr2
