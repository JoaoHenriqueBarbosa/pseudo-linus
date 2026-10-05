mk r
git branch -u origin/nonex main 2>&1 | cat -A | head -4
git branch -u 2>&1 | head -3
git branch -u main nonexist x 2>&1 | head -3
git branch --unset-upstream nonex 2>&1
git branch 'bad..name' 2>&1 | cat -A
git branch --set-upstream-to=main 2>&1 | head -3
git branch -u main 2>&1 | head -3
git branch -u HEAD 2>&1
git checkout -q --detach
git branch -u main 2>&1
git branch --unset-upstream 2>&1
git branch -m x 2>&1
git branch -c x 2>&1
