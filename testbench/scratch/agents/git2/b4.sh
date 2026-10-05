mk r
git remote add origin /nonexistent
git update-ref refs/remotes/origin/main HEAD~1
git update-ref refs/remotes/origin/dev HEAD~2
git update-ref refs/remotes/origin/gone HEAD~2
git config branch.main.remote origin
git config branch.main.merge refs/heads/main
git branch other HEAD~2
git branch ahead2 HEAD
git config branch.ahead2.remote origin
git config branch.ahead2.merge refs/heads/dev
git branch behind origin/dev
git config branch.behind.remote origin
git config branch.behind.merge refs/heads/main
git branch gone1 HEAD
git config branch.gone1.remote origin
git config branch.gone1.merge refs/heads/zzz
git branch same origin/main
git config branch.same.remote origin
git config branch.same.merge refs/heads/main
cat .git/config
ex git branch -vv
ex git branch -v
ex git branch -t t1 origin/dev
ex git branch --track t2 origin/dev
ex git branch --track=inherit t3 origin/dev
ex git branch --no-track t4 origin/dev
ex git branch --set-upstream-to=origin/dev other
ex git branch -u origin/main other
ex git branch -u main other
ex git branch --unset-upstream other
cat .git/config
ex git branch -u origin/dev
ex git branch -u origin/dev nonexist
ex git checkout -b t5 origin/dev
ex git checkout -b t6 --no-track origin/dev
ex git switch -c t7 origin/dev
ex git switch -c t8 -t main
ex git switch t5
ex git switch main
git config branch.autoSetupMerge always
ex git branch t9 main
ex git branch t10 origin/main
cat .git/config
git checkout -q main
ex git branch -d t1
ex git branch -vv --list 'ahead*'
