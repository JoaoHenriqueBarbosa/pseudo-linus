mk r
git branch old HEAD~1
echo "tree: $(git rev-parse --short old^{tree}) commit: $(git rev-parse --short old)"
ex2 git checkout old b.txt
ex2 git checkout old -- c.txt
echo X > b.txt
ex2 git checkout old b.txt
ex2 git checkout old -q b.txt
