mk r
git update-ref refs/remotes/origin/main HEAD~1
git update-ref refs/remotes/origin/dev HEAD~2
git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
git config branch.main.remote origin
git config branch.main.merge refs/heads/main
git branch other HEAD~2
ex git branch -r
ex git branch -a
ex git branch -vv
ex git branch -v -r
ex git branch -vva
ex git branch -avv
ex git branch --all -v
ex git branch -r -v --list 'origin/d*'
ex git branch -l 'o*'
ex git branch -r -l 'origin/*'
ex git branch --list -r 'o*'
ex git branch -t other2 origin/dev
cat .git/config
ex git branch --set-upstream-to=origin/dev other
ex git branch -u origin/dev other
ex git branch -u origin/nonex other
ex git branch --unset-upstream other
ex git branch --unset-upstream other
git branch -vv
ex git branch -d -r origin/dev
ex git branch -r
git update-ref refs/remotes/origin/dev HEAD~2
ex git branch -dr origin/dev
ex git branch -r -D origin/dev
ex git branch --points-at HEAD
ex git branch --points-at HEAD~2
ex git branch --sort=-committerdate
ex git branch --sort=refname
ex git branch --format='%(refname:short) %(objectname:short) %(HEAD)'
ex git branch --contains HEAD~2 other
ex git branch --abbrev=12 -v
ex git branch --no-abbrev -v
ex git branch -q newq
ex git branch -qd newq
ex git branch -v --no-color
ex git branch --column
ex git branch -i -l 'OTH*'
ex git branch -l 'OTH*'
ex git branch -l main other
