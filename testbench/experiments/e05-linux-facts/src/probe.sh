# Sonda rodada dentro do oráculo (Debian 13). Cada seção começa com "@@nome" numa linha própria.
# Não usa nada não determinístico na saída que a bancada compara (pids e horários vão só pras amostras).
set +e
sec() { printf '\n@@%s\n' "$1"; }

sec getconf
for v in PATH_MAX NAME_MAX PIPE_BUF; do printf '%s=%s\n' "$v" "$(getconf "$v" /)"; done
printf 'ARG_MAX=%s\n' "$(getconf ARG_MAX)"
printf 'CLK_TCK=%s\n' "$(getconf CLK_TCK)"
printf 'PAGESIZE=%s\n' "$(getconf PAGESIZE)"

sec pid_max
cat /proc/sys/kernel/pid_max

sec umask
umask

sec ulimit
ulimit -a

sec uname
uname -s; uname -m; uname -o

sec kill_l
kill -l

sec exit_codes
bash -c 'kill -TERM $$'; echo "TERM=$?"
bash -c 'kill -KILL $$'; echo "KILL=$?"
bash -c 'kill -INT $$'; echo "INT=$?"
bash -c 'yes | head -c 1 >/dev/null; echo "PIPESTATUS=${PIPESTATUS[*]}"'
bash -c 'nonexistent_command_xyz' 2>/dev/null; echo "NOTFOUND=$?"
touch noexec; bash -c './noexec' 2>/dev/null; echo "NOEXEC=$?"

sec errors
cat missing 2>&1
mkdir d; mkdir d 2>&1
touch f; cd f 2>&1
mkdir -p e/x; rmdir e 2>&1
cat d 2>&1
ln -s loop1 loop2; ln -s loop2 loop1; cat loop1 2>&1
cat f/x 2>&1
echo x 2>&1 >/dev/full
ls "$(printf 'a%.0s' $(seq 256))" 2>&1
rm d 2>&1
mv missing x 2>&1
cp missing x 2>&1
rmdir missing 2>&1
ln -s f f 2>&1

sec root_bypass
chmod 000 f; cat f >/dev/null 2>&1; echo "cat_mode_000_as_root=$?"
mkdir locked; chmod 000 locked; touch locked/inside 2>&1; echo "write_into_mode_000_dir_as_root=$?"

sec ls_time
touch -d '-5 months' recent; touch -d '-7 months' old; touch -d '+2 days' future
ls -l recent old future | awk '{print $6, $7, $8, $9}'

sec proc_status
cat /proc/self/status
sec proc_stat
cat /proc/self/stat
sec proc_meminfo
cat /proc/meminfo
sec proc_loadavg
cat /proc/loadavg
sec proc_uptime
cat /proc/uptime
sec proc_limits
cat /proc/self/limits
sec proc_cpuinfo
awk 'BEGIN{RS=""} NR==1{print; exit}' /proc/cpuinfo
sec proc_cmdline
cat /proc/self/cmdline | od -An -c
sec proc_mounts
cat /proc/self/mounts
