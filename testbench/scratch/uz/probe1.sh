for c in "unzip" "unzip -h" "unzip -hh" "unzip -v" "unzip -q" "unzip -Z" "unzip -Z -h" "unzip -vqqqq" "unzip -d" "unzip -x z.zip" "unzip -cu z.zip" "unzip -no z.zip" "unzip -k z.zip" "unzip -a -Z" "UNZIP='-o \"a b\"' unzip -v"; do
  echo "=== $c"; eval "$c" 2>&1 | md5sum; echo "rc=${PIPESTATUS[0]}"
done
