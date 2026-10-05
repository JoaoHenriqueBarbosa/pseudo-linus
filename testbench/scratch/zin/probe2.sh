cd /tmp; mkdir p2; cd p2
r() { echo "\$ $*"; "$@" </dev/null 2>&1; echo "[exit $?]"; }
seq 1 20000 > s.txt; printf 'hello\n' > h.txt; : > e.txt
zstd -q -k s.txt; zstd -q -k h.txt; zstd -q -k e.txt
r zstd -l s.txt.zst h.txt.zst e.txt.zst; r zstd -lv s.txt.zst e.txt.zst
cat s.txt.zst h.txt.zst > m.zst; r zstd -l m.zst; r zstd -dc m.zst | tail -c 20; r zstd -lv m.zst
# corrupções
head -c 30 s.txt.zst > c1.zst; printf 'XX' >> c1.zst; tail -c +33 s.txt.zst >> c1.zst
r zstd -t c1.zst; r zstd -dc c1.zst | wc -c
head -c -1 s.txt.zst > c2.zst; r zstd -t c2.zst; r zstd -dc c2.zst | wc -c
head -c -4 s.txt.zst > c3.zst; printf 'abcd' >> c3.zst; r zstd -t c3.zst; r zstd -dc c3.zst | wc -c
r zstd -t --no-check c3.zst
head -c 3 s.txt.zst > c4.zst; r zstd -t c4.zst; r zstd -l c4.zst
printf '\x28\xb5\x2f\xfd\x08' > c5.zst; r zstd -t c5.zst
printf '\x28\xb5\x2f\xfd\x00\xff\x00\x00\x00' > c6.zst; r zstd -t c6.zst
printf '\x50\x2a\x4d\x18\x03\x00\x00\x00abc' > sk.zst; cat sk.zst h.txt.zst > skh.zst; r zstd -dc skh.zst; r zstd -l skh.zst
head -c 6 h.txt.zst > c7.zst; r zstd -dc c7.zst; r zstd -l c7.zst
cat h.txt.zst > g1.zst; printf 'garbage' >> g1.zst; r zstd -dc g1.zst; r zstd -l g1.zst
# janela
zstd -q --long=27 -f -o big.zst s.txt; r zstd -l big.zst
r zstd -t -M1KB s.txt.zst; r zstd -t --memory=1MB big.zst
# outros formatos
gzip -c h.txt > h.gz; xz -c h.txt > h.xz; xz --format=lzma -c h.txt > h.lzma
r zstd -dc h.gz; r zstd -dc h.xz; r zstd -dc h.lzma; r zstd -d -f h.gz; ls h.*
r zstd -q --format=gzip -f h.txt; r gzip -dc h.txt.gz; r zstd -q --format=xz -f h.txt; r xz -dc h.txt.xz
r zstd -q --format=lzma -f h.txt; r xz -dc h.txt.lzma; r zstd -q --format=lz4 -f h.txt; r zstd -dc h.txt.lz4
head -c 15 h.gz > hg.gz; r zstd -dc hg.gz; head -c 20 h.xz > hx.xz; r zstd -dc hx.xz
# nomes
cp s.txt.zst t.tzst; r zstd -d t.tzst; ls t.*; cp s.txt.zst u.zstd; r zstd -d u.zstd; ls u*
r zstd -d s.txt; r zstd -d nosuffix; r zstd -d .zst
mkdir od; r zstd -q --output-dir-flat od h.txt s.txt; ls od; r zstd -d --output-dir-flat od -f od/h.txt.zst; ls od
mkdir -p sub/x; cp h.txt sub/x/; r zstd -q -r --output-dir-mirror mir sub; ls -R mir
printf 'h.txt\ns.txt\n' > list; r zstd -q -f --filelist list; r zstd -q -f --filelist nolist
r zstd -q --exclude-compressed h.txt.zst; ls h.txt.zst*
# sobrescrita e prompts
r zstd h.txt; r zstd -q h.txt; echo y | zstd h.txt 2>&1; echo "[y $?]"; r zstd -o s.txt h.txt
r zstd -o x.zst h.txt s.txt; r zstd -q -o x.zst h.txt s.txt; r zstd -f -o x.zst h.txt s.txt; r zstd -dc x.zst | wc -c
r zstd --rm -c h.txt | wc -c; ls h.txt; cp h.txt hr.txt; r zstd -q --rm hr.txt; ls hr*
r zstd -q -d --rm hr.txt.zst; ls hr*
# opções
r zstd --show-default-cparams -c s.txt | wc -c; r zstd -19 --show-default-cparams -c h.txt | wc -c
r zstd -vvv -c h.txt | wc -c
r zstd --zstd=wlog=20 -c h.txt | wc -c; r zstd --zstd=bogus=1 h.txt; r zstd --fast=0 h.txt; r zstd --fast=3 -q -c h.txt | wc -c
r zstd --long=x h.txt; r zstd -o; r zstd -o -c h.txt; r zstd --threads -c h.txt; r zstd --threads=abc h.txt
r zstd -99999999999 h.txt; r zstd --ultra -25 -q -c h.txt | wc -c; r zstd -b1 -bogus
r zstd --single-thread --rsyncable h.txt; r zstd --single-thread -B1M -q -f h.txt
ZSTD_CLEVEL=abc zstd -q -c h.txt | wc -c; ZSTD_CLEVEL=99999999999 zstd -q -c h.txt | wc -c
r zstdcat h.txt; r zstdcat h.txt.zst s.txt.zst | wc -c; r unzstd -q -f h.txt.zst; ls h.txt*
r zstd -t h.txt.zst s.txt.zst c1.zst; r zstd -d -c --pass-through h.txt
r zstd -l -; r zstd -l; r zstd -c -; r zstd nope1 nope2
r zstd -D nodict -c h.txt; r zstd -D h.txt -c h.txt
