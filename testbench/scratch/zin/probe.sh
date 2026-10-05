cd /tmp; printf 'hello\n' > a.txt; seq 1 100 > b.txt
r() { echo "\$ $*"; "$@" </dev/null 2>&1; echo "[exit $?]"; }
r zstd a.txt; ls -la a.txt*; r zstd a.txt; r zstd -f a.txt; r zstd -q a.txt -o out.zst; r zstd nope.txt
r zstd -d a.txt.zst; r zstd -df a.txt.zst; ls; r zstd -d nope.zst; r zstd -t a.txt.zst; r zstd -t a.txt
r zstd -l a.txt.zst; r zstd -lv a.txt.zst; r zstd --rm b.txt; ls
r zstd -d --rm b.txt.zst; ls
r zstd -k b.txt; r zstd -v -f b.txt; r zstd -19 -f b.txt; r zstd -q -f --format=gzip b.txt
echo hi | zstd | zstd -d; echo "[pipe $?]"
r zstd -d a.txt; r zstd -V; r unzstd -f a.txt.zst; r zstdcat a.txt.zst; r zstd -dc a.txt.zst b.txt.zst
r zstd -x a.txt; r zstd -h; r zstd -H; r unzstd -H; r zstdcat -h
printf 'x' | zstd > /dev/null; echo "[stdin-ok $?]"; zstd a.txt </dev/null 2>&1; echo "[tty $?]"
mkdir d; r zstd d; cp a.txt d/; r zstd -r d; ls d
r zstd -q -f a.txt b.txt; r zstd -f a.txt b.txt; r zstd -d -f a.txt.zst b.txt.zst
r zstd -c a.txt a.txt
printf 'garbage' > g.zst; r zstd -d g.zst; r zstd -t g.zst; r zstd -l g.zst
head -c 10 a.txt.zst > t.zst; r zstd -d t.zst; r zstd -dc t.zst
r zstd --long a.txt -f -q; r zstd -T0 -f -q a.txt; r zstd --ultra -22 -f -q a.txt; r zstd -20 -f a.txt; r zstd -0 -f -q a.txt
r zstd --no-check -q -c a.txt; r zstd --fast -q -f a.txt; r zstd -o x.zst -q -f a.txt b.txt
