# Backlog do pseudo-linus

Inventário de 2026-10-05: o que o `/usr/bin` e o `/usr/sbin` do oráculo (Debian 13, imagem
`pseudo-linus-oracle`) têm e o sandbox ainda não. A fila de agentes sai daqui, em tarefas curtas.

## Scripts (rodam no nosso `sh` copiados como estão, depois de existir o interpretador)

- **sh:** bzdiff, bzexe, bzgrep, bzmore, dpkg-maintscript-helper, gettext.sh, gzexe, installkernel,
  invoke-rc.d, lesspipe, pam_namespace_helper, policy-rc.d, service, shadowconfig, tarcat,
  update-ca-certificates, update-shells, wcurl, which.debianutils, xzdiff, xzgrep, xzless, xzmore,
  zcmp, zdiff, zegrep, zfgrep, zforce, zgrep, zipgrep, zless, zmore, znew, zstdgrep, zstdless,
  bashbug, gawkbug.
- **bash:** ldd, tzselect.
- **perl (depende de portar o perl):** corelist, cpan, c_rehash, debconf*, deb-systemd-*,
  dpkg-preconfigure, dpkg-reconfigure, enc2xs, encguess, h2ph, h2xs, instmodsh, json_pp,
  libnetcfg, pam-auth-update, pam_getenv, perlbug, perldoc, perlivp, perlthanks, piconv, pl2pm,
  pod2*, podchecker, prove, ptar, ptardiff, ptargrep, shasum, splain, streamzip, update-rc.d,
  xsubpp, zipdetails.
- **python3 (depende do port em docs/python3-port.md):** py3clean, py3compile, pydoc3.13,
  pygettext3.13, tomlq, xq-python, register-python-argcomplete e afins.

## Links (aparecem quando o alvo existir)

bzcmp, bzegrep, bzfgrep, bzless, captoinfo, infotocap, dnsdomainname, domainname,
nisdomainname, ypdomainname, getty, git-receive-pack, git-upload-archive, git-upload-pack,
i386, linux32, linux64, x86_64, lessfile, lz* (alternatives do lzip), nawk, pager, pstree.x11,
python3, pydoc3, pygettext3, pdb3, rbash, rmt, sg, snice, uncompress, vigr, xzcmp, xzegrep,
xzfgrep, e os do binutils (ar, as, ld, nm, objdump, readelf, size, strip, c++filt...).

## Binários a portar

- **coreutils:** pinky, shred.
- **util-linux:** blkid, blockdev, chcpu, chmem, choom, chrt, fallocate, findfs, findmnt, flock,
  fsck, fstrim, ionice, ipcmk, ipcrm, ipcs, isosize, logger, losetup, lsblk, lscpu, lsipc,
  lslocks, lslogins, lsmem, lsns, mkfs, mkswap, mount, mountpoint, nsenter, partx, prlimit,
  readprofile, renice, rtcwake, script, scriptlive, scriptreplay, setarch, setpriv, setsid,
  setterm, swapon, swapoff, swaplabel, taskset, uclampset, umount, unshare, wall, wdctl,
  wipefs, zramctl, agetty, login, nologin, runuser, su, sulogin, switch_root, pivot_root,
  ldattach, blkdiscard, blkzone, fsfreeze, chfn, chsh, newgrp, vipw.
- **procps:** pmap, pwdx, skill, slabtop, sysctl, tload, vmstat, w, pstree, peekfd, prtstat,
  pslog, fuser, killall5.
- **shadow:** chage, chgpasswd, chpasswd, expiry, gpasswd, groupadd, groupdel, groupmod, grpck,
  grpconv, grpunconv, newusers, passwd, pwck, pwconv, pwunconv, useradd, userdel, usermod.
- **outros:** dash, dc, mawk, iconv, iconvconfig, localedef, ldconfig, pldd, tic, zdump, zic,
  stty, flock, faketime, lessecho, lesskey, lzmainfo, pzstd, bzip2recover, funzip, unzipsfx,
  zipcloak, zipnote, zipsplit, curl, wget, openssl, sqv, git-shell, scalar, busybox,
  start-stop-daemon, update-alternatives, update-passwd, fstab-decode, clear_console,
  mkhomedir_helper, dpkg e família, apt e família, perl, python3.13, binutils, gprofng.
- **Fora do sandbox por natureza (decidir caso a caso):** pam_timestamp_check, unix_chkpwd,
  unix_update, pwhistory_helper, faillock, chcon, runcon.
