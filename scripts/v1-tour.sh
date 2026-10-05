# Passeio rápido pela v1 do pseudo-linus. Roda dentro do sandbox:
#   target/release/osh scripts/v1-tour.sh
# (o osh lê o script do host e executa no sh do sandbox; nada toca o host)

echo "== sistema"
cat /etc/os-release | head -2
uname -a
id

echo "== arquivos e texto"
cd /work
printf 'banana\nmaçã\nabacaxi\nbanana\n' > frutas.txt
sort frutas.txt | uniq -c
wc -l frutas.txt
sed 's/banana/BANANA/' frutas.txt | grep -n A
awk '{ n += length($0) } END { print "bytes de texto:", n }' frutas.txt
xxd frutas.txt | head -2

echo "== git"
export GIT_AUTHOR_NAME=Você GIT_AUTHOR_EMAIL=voce@exemplo.com GIT_COMMITTER_NAME=Você GIT_COMMITTER_EMAIL=voce@exemplo.com
git init -q -b main repo && cd repo
echo um > a.txt && git add a.txt && git commit -qm "primeiro"
git switch -qc tema && echo dois >> a.txt && git commit -qam "segundo"
git switch -q main && git merge -q tema && git log --oneline
git diff HEAD~1 --stat
cd /work

echo "== compactação"
tar czf frutas.tgz frutas.txt && tar tzvf frutas.tgz
zip -q f.zip frutas.txt && unzip -l f.zip

echo "== dados"
echo '{"nome":"pseudo-linus","versao":1}' | jq .nome
sqlite3 :memory: "create table t(x); insert into t values (1),(2),(3); select sum(x) from t;"
echo '2 ^ 64' | bc

echo "== processos e terminal"
sleep 5 & ps -o pid,stat,comm
kill %1
TERM=xterm tput cols; TERM=xterm infocmp -1 xterm | head -3
date -d @0 -u
