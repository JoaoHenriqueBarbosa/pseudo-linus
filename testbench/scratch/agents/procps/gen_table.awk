# Gera as tabelas do ps (format_array, macro_array) em Rust a partir do output.c do procps-ng 4.0.4.
# Uso: awk -v mode=fmt|macro|items -f gen_table.awk output.c

function camel(s,    n, parts, i, r) {
    n = split(tolower(s), parts, "_")
    r = ""
    for (i = 1; i <= n; i++) r = r toupper(substr(parts[i], 1, 1)) substr(parts[i], 2)
    return r
}

function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }

/^static const format_struct format_array\[\]/ { infmt = 1; next }
/^static const macro_struct macro_array\[\]/ { inmac = 1; next }
/^};/ { infmt = 0; inmac = 0 }

infmt && /^\{"/ {
    line = $0
    sub(/\/\*.*$/, "", line)
    sub(/\},[ \t]*$/, "", line)
    sub(/^\{/, "", line)
    gsub(/\(int\)\(2\*sizeof\(long\)\)/, "16", line)
    # nome e cabeçalho entre aspas, depois o resto sem vírgulas dentro
    match(line, /^"[^"]*"/); spec = substr(line, 2, RLENGTH - 2); rest = substr(line, RLENGTH + 1)
    sub(/^[ \t]*,[ \t]*/, "", rest)
    match(rest, /^"[^"]*"/); head = substr(rest, 2, RLENGTH - 2); rest = substr(rest, RLENGTH + 1)
    sub(/^[ \t]*,[ \t]*/, "", rest)
    n = split(rest, f, ",")
    pr = trim(f[1]); sr = trim(f[2]); width = trim(f[3]); vendor = trim(f[4]); flags = trim(f[5])
    if (vendor == "LNx") vendor = "LNX"
    gsub(/\|/, " | ", flags)
    sub(/^PIDS_/, "", sr)
    item = camel(sr)
    items[item] = 1
    nop = (pr == "pr_nop") ? "true" : "false"
    printf "    Fmt { spec: \"%s\", head: \"%s\", pr: %s, nop: %s, sr: Item::%s, width: %s, vendor: %s, flags: %s },\n", spec, head, pr, nop, item, width, vendor, flags > "/dev/stderr"
    next
}

inmac && /^\{"/ {
    line = $0
    sub(/\/\*.*$/, "", line)
    sub(/\}[ \t]*,?[ \t]*$/, "", line)
    sub(/^\{/, "", line)
    match(line, /^"[^"]*"/); spec = substr(line, 2, RLENGTH - 2); rest = substr(line, RLENGTH + 1)
    sub(/^[ \t]*,[ \t]*/, "", rest)
    match(rest, /^"[^"]*"/); body = substr(rest, 2, RLENGTH - 2)
    printf "    (\"%s\", \"%s\"),\n", spec, body > "/dev/stdout"
    next
}

END {
    for (k in items) print k > "/dev/stdout"
}
