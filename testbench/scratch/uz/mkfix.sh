#!/bin/bash
# Gera as fixtures da sonda do unzip com o zip 3.0 do oráculo e imprime cada uma como
# "nome base64" (uma por linha).
set -e
export TZ=UTC
cd /tmp && rm -rf fx && mkdir fx && cd fx
mkdir -p src/sub src/dir2
printf 'hello world\n' > src/a.txt
printf 'deep\n' > src/sub/deep.txt
printf '#!/bin/sh\necho hi\n' > src/exec.sh && chmod 755 src/exec.sh
head -c 20000 /dev/zero | tr '\0' 'y' > src/big.txt
ln -s a.txt src/link
touch -d '2026-01-15 12:00:00' src/a.txt src/sub/deep.txt src/exec.sh src/big.txt src/sub src/dir2 src
touch -h -d '2026-01-15 12:00:00' src/link
cd src
zip -q -r -y ../t1.zip a.txt sub exec.sh link big.txt dir2
echo 'archive comment line' | zip -q -z ../t1.zip
zip -q -0 -r ../stored.zip a.txt sub
zip -q -X ../nox.zip a.txt big.txt
zip -q -Z bzip2 ../bz.zip a.txt big.txt
zip -q -P secret ../enc.zip a.txt big.txt
zip -q -1 ../fast.zip big.txt
zip -q -9 ../best.zip big.txt
printf 'x\n' > "$(printf 'ctl\001name.txt')"
printf 'u\n' > 'ação.txt'
zip -q ../names.zip "$(printf 'ctl\001name.txt')" 'ação.txt'
cd ..
printf 'PK\005\006\000\000\000\000\000\000\000\000\000\000\000\000\000\000\000\000\000\000' > empty.zip
{ printf 'GARBAGEPREFIX'; cat t1.zip; } > pre.zip
head -c 200 t1.zip > trunc.zip
head -c $(( $(stat -c %s t1.zip) - 10 )) t1.zip > trunc2.zip
printf 'not a zip at all\n' > notzip.zip
for f in t1 stored nox bz enc fast best names empty pre trunc trunc2 notzip; do
  echo "$f $(base64 -w0 $f.zip)"
done
