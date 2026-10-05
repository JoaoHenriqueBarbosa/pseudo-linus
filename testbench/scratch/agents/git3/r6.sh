mk r6
git remote add o /a
git remote set-url --add o /b
ex2 git remote set-url o /c
git config --get-all remote.o.url
ex2 git remote set-url o /c '^/a'
git config --get-all remote.o.url
ex2 git remote set-url --delete o '^/'
ex2 git remote set-url --delete o '('
ex2 git remote set-url o /z '('
ex2 git remote set-url --add o
ex2 git remote get-url --push o
ex2 git remote get-url o extra
ex2 git remote get-url --all --push o
git config --add branch.main.merge refs/heads/y
git config branch.main.remote o
git config --add branch.main.merge refs/heads/z
ex2 git remote rm o
git config --get-regexp 'branch|remote'
git remote add fo /fo
git config remote.fo.pushurl /pu
git config url.https://github.com/.insteadOf gh:
git config url.ssh://git@github.com/.pushInsteadOf https://github.com/
git remote add gh gh:me/r.git
ex2 git remote -v
ex2 git remote get-url gh
ex2 git remote get-url --push gh
git config remote.only.fetch '+refs/heads/*:refs/remotes/only/*'
ex2 git remote
ex2 git remote -v
ex2 git remote show -n only
ex2 git remote add only /o
ex2 git remote set-url only /o
ex2 git remote get-url only
ex2 git remote rm only
cat .git/config
git config remote.empty.url ''
ex2 git remote -v
ex2 git remote get-url empty
ex2 git remote show -n empty
git remote add 'sp ace' /x
git remote add -- -dash /x
git remote add .dot /x
git remote add 'a..b' /x
git remote add 'a.lock' /x
git remote add 'a~b' /x
git remote add 'ünï' /x
git remote add '' /x
git remote add 'a:b' /x
git remote add 'a@{b' /x
git remote add '@' /x
git remote add 'end/' /x
git remote add 'x.' /x
ex2 git remote
ex2 git remote rename gh gh2
ex2 git remote rename gh2 gh2
git remote add noref /n
ex2 git remote rename noref nr
git config --get-regexp remote.nr
git remote add -t a -t b multi /m
ex2 git remote rename multi m2
git config --get-regexp remote.m2
git config remote.m2.fetch '+refs/heads/c:refs/remotes/other/c'
ex2 git remote rename m2 m3
cat .git/config
