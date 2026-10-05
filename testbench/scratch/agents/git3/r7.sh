mk r7
git remote add h https://nonexistent.invalid/x.git
git remote add s ssh://git@nonexistent.invalid/x.git
git remote add g git://nonexistent.invalid/x.git
git remote add sc git@nonexistent.invalid:x/y.git
git remote add l /tmp/notrepo
mkdir -p /tmp/notrepo
git remote add emp ''
ex2 git remote show h
ex2 git remote show s
ex2 git remote show g
ex2 git remote show sc
ex2 git remote show l
ex2 git remote show emp
ex2 git remote update unknown
ex2 git remote update h l
git config remotes.grp 'l s'
ex2 git remote update grp
ex2 git remote prune l
ex2 git remote prune l h
ex2 git remote -n
ex2 git remote prune --dry-run l
ex2 git remote prune -n l
