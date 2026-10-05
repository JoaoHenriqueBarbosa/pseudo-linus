mk r
for i in $(seq 1 23); do git branch br$i; done
git branch long-branch-name-here
ex git branch --column
ex git branch --column=row
ex git branch --column=dense
ex git branch --column=plain
COLUMNS=40 git branch --column=row,dense
ex git branch --column=foo
ex git branch --column='row bogus x'
ex git branch -v --column
ex git branch --no-column
git -c column.ui=always branch | head -3
git -c column.ui=bogus branch; echo "[exit=$?]"
git -c column.ui branch; echo "[exit=$?]"
git branch --column=never | head -2
git -c column.branch=row branch | head -2
COLUMNS=10 git branch --column | head -3
git config column.ui "row nodense,always"; git branch --column=dense
git config column.ui bogus; git branch; echo "[exit=$?]"
git config --unset column.ui
git branch --column=' , '; echo "[exit=$?]"
ex git branch --column --format='%(refname:short)'
ex git branch --column -r
for i in $(seq 1 23); do git tag t$i; done
ex git tag --column
ex git tag -l --column=row 't1*'
ex git tag --column=foo
ex git tag -n --column
ex git tag -n --column=never
git -c column.tag=always tag -n | head -2
ex git tag --column x
ex git tag -d --column x
ex git tag --column --format='%(refname:short)%0a%(objecttype)' 't2*'
