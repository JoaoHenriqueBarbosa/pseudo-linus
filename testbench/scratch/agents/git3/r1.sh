mk r1
ex2 git remote
ex2 git remote -v
ex2 git remote add origin /x/y.git
ex2 git remote
ex2 git remote -v
ex2 git config -l --local
cat .git/config
ex2 git remote add origin /z
ex2 git remote add 'bad name' /z
ex2 git remote add a/b /z
ex2 git remote add
ex2 git remote add x
ex2 git remote add -t main -t dev two /two.git
cat .git/config
ex2 git remote add -m main three /three.git
cat .git/config
ex2 git remote add --mirror=push four /four.git
ex2 git remote add --mirror=fetch five /five.git
ex2 git remote add --mirror six /six.git
ex2 git remote add --tags seven /seven.git
ex2 git remote add --no-tags eight /eight.git
ex2 git remote add --mirror=push -t x nine /n.git
ex2 git remote add --mirror=push -m x nine /n.git
ex2 git remote add --mirror=bogus nine /n.git
cat .git/config
ex2 git remote -v
ex2 git remote get-url origin
ex2 git remote get-url nope
ex2 git remote get-url --push origin
ex2 git remote get-url --all origin
ex2 git remote set-url origin /new.git
ex2 git remote set-url --add origin /extra.git
ex2 git remote get-url --all origin
ex2 git remote -v
ex2 git remote set-url --push origin /push.git
ex2 git remote -v
ex2 git remote get-url --push --all origin
ex2 git remote set-url --delete origin /extra.git
ex2 git remote set-url --delete origin /new.git
ex2 git remote set-url origin /new2.git /zzz
ex2 git remote set-url origin /new3.git '^/new'
ex2 git remote get-url --all origin
ex2 git remote set-url nope /a
ex2 git remote set-url
ex2 git remote set-url --add --delete origin /a
ex2 git remote set-url --delete --push origin /push.git
ex2 git remote get-url --push origin
cat .git/config
