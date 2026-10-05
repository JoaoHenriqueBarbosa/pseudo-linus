mk r
git branch x
git branch y main
git checkout -q -b z
git switch -q -c w
git branch -f x HEAD~1
git checkout -q main
git checkout -q -B x main
git checkout -q -B nb HEAD~1
git switch -q -C nb2
git branch --create-reflog zz HEAD~1
git checkout -q --detach
git branch dd
git checkout -q -b fromdet
git branch --edit-description 2>&1
for b in x y z w nb nb2 zz dd fromdet; do echo "== $b"; cat .git/logs/refs/heads/$b | cut -c83-; done
echo == HEAD
cat .git/logs/HEAD | cut -c83-
