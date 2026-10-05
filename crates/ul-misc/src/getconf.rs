//! `getconf` da glibc 2.41 (Debian 13), portado de `posix/getconf.c`: as variáveis do sistema
//! (`sysconf`), de caminho (`pathconf`) e de configuração de string (`confstr`), na ordem da tabela
//! `vars[]` do original, com `-a`, `-v SPEC`, `--help`, `--version` e as mensagens de erro e uso.
//!
//! Os valores que no Linux dependem da máquina saem do "kernel" do sandbox (`sysabi`): os limites de
//! recursos (`ARG_MAX`, `OPEN_MAX`, `CHILD_MAX`, `SIGQUEUE_MAX`), o número de CPUs, as páginas de memória
//! (`/proc/meminfo`) e o `statfs` do caminho (`NAME_MAX`, `LINK_MAX`, `POSIX_REC_*`...). O resto é
//! constante do x86_64 da glibc e vem do oráculo. Os tamanhos de cache (`LEVEL1_*` a `LEVEL4_*`) vêm
//! do `cpuid` na glibc e aqui são os valores do oráculo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, FileType, RLIM_INFINITY, Resource, sys};

use crate::util::io;

/// Nomes do `pathconf` que o getconf usa.
#[derive(Copy, Clone)]
enum Pc {
    LinkMax,
    MaxCanon,
    MaxInput,
    NameMax,
    PathMax,
    PipeBuf,
    SockMaxbuf,
    AsyncIo,
    ChownRestricted,
    NoTrunc,
    PrioIo,
    SyncIo,
    Vdisable,
    FileSizeBits,
    AllocSizeMin,
    RecIncrXferSize,
    RecMaxXferSize,
    RecMinXferSize,
    RecXferAlign,
    SymlinkMax,
    Symlinks2,
}

/// O valor de uma variável do `sysconf`: constante, indefinida (-1) ou lida do sistema.
#[derive(Copy, Clone)]
enum Val {
    N(i64),
    /// `-1` do `sysconf`: "undefined" no getconf.
    U,
    /// `sysconf` devolve `ULONG_MAX` como `-1` e o getconf imprime sem sinal.
    UlongMax,
    ArgMax,
    ChildMax,
    OpenMax,
    NgroupsMax,
    SigqueueMax,
    NprocConf,
    NprocOnln,
    PhysPages,
    AvphysPages,
}

#[derive(Copy, Clone)]
enum Call {
    Path(Pc),
    Sys(Val),
    /// `confstr`: o valor é fixo.
    Conf(&'static str),
}

/// A tabela `vars[]` do getconf.c, na ordem do original.
const VARS: &[(&str, Call)] = &[
    ("LINK_MAX", Call::Path(Pc::LinkMax)),
    ("_POSIX_LINK_MAX", Call::Path(Pc::LinkMax)),
    ("MAX_CANON", Call::Path(Pc::MaxCanon)),
    ("_POSIX_MAX_CANON", Call::Path(Pc::MaxCanon)),
    ("MAX_INPUT", Call::Path(Pc::MaxInput)),
    ("_POSIX_MAX_INPUT", Call::Path(Pc::MaxInput)),
    ("NAME_MAX", Call::Path(Pc::NameMax)),
    ("_POSIX_NAME_MAX", Call::Path(Pc::NameMax)),
    ("PATH_MAX", Call::Path(Pc::PathMax)),
    ("_POSIX_PATH_MAX", Call::Path(Pc::PathMax)),
    ("PIPE_BUF", Call::Path(Pc::PipeBuf)),
    ("_POSIX_PIPE_BUF", Call::Path(Pc::PipeBuf)),
    ("SOCK_MAXBUF", Call::Path(Pc::SockMaxbuf)),
    ("_POSIX_ASYNC_IO", Call::Path(Pc::AsyncIo)),
    ("_POSIX_CHOWN_RESTRICTED", Call::Path(Pc::ChownRestricted)),
    ("_POSIX_NO_TRUNC", Call::Path(Pc::NoTrunc)),
    ("_POSIX_PRIO_IO", Call::Path(Pc::PrioIo)),
    ("_POSIX_SYNC_IO", Call::Path(Pc::SyncIo)),
    ("_POSIX_VDISABLE", Call::Path(Pc::Vdisable)),
    ("ARG_MAX", Call::Sys(Val::ArgMax)),
    ("ATEXIT_MAX", Call::Sys(Val::N(2147483647))),
    ("CHAR_BIT", Call::Sys(Val::N(8))),
    ("CHAR_MAX", Call::Sys(Val::N(127))),
    ("CHAR_MIN", Call::Sys(Val::N(-128))),
    ("CHILD_MAX", Call::Sys(Val::ChildMax)),
    ("CLK_TCK", Call::Sys(Val::N(100))),
    ("INT_MAX", Call::Sys(Val::N(2147483647))),
    ("INT_MIN", Call::Sys(Val::N(-2147483648))),
    ("IOV_MAX", Call::Sys(Val::N(1024))),
    ("LOGNAME_MAX", Call::Sys(Val::N(256))),
    ("LONG_BIT", Call::Sys(Val::N(64))),
    ("MB_LEN_MAX", Call::Sys(Val::N(16))),
    ("NGROUPS_MAX", Call::Sys(Val::NgroupsMax)),
    ("NL_ARGMAX", Call::Sys(Val::N(4096))),
    ("NL_LANGMAX", Call::Sys(Val::N(2048))),
    ("NL_MSGMAX", Call::Sys(Val::N(2147483647))),
    ("NL_NMAX", Call::Sys(Val::N(2147483647))),
    ("NL_SETMAX", Call::Sys(Val::N(2147483647))),
    ("NL_TEXTMAX", Call::Sys(Val::N(2147483647))),
    ("NSS_BUFLEN_GROUP", Call::Sys(Val::N(1024))),
    ("NSS_BUFLEN_PASSWD", Call::Sys(Val::N(1024))),
    ("NZERO", Call::Sys(Val::N(20))),
    ("OPEN_MAX", Call::Sys(Val::OpenMax)),
    ("PAGESIZE", Call::Sys(Val::N(4096))),
    ("PAGE_SIZE", Call::Sys(Val::N(4096))),
    ("PASS_MAX", Call::Sys(Val::N(8192))),
    ("PTHREAD_DESTRUCTOR_ITERATIONS", Call::Sys(Val::N(4))),
    ("PTHREAD_KEYS_MAX", Call::Sys(Val::N(1024))),
    ("PTHREAD_STACK_MIN", Call::Sys(Val::N(16384))),
    ("PTHREAD_THREADS_MAX", Call::Sys(Val::U)),
    ("SCHAR_MAX", Call::Sys(Val::N(127))),
    ("SCHAR_MIN", Call::Sys(Val::N(-128))),
    ("SHRT_MAX", Call::Sys(Val::N(32767))),
    ("SHRT_MIN", Call::Sys(Val::N(-32768))),
    ("SSIZE_MAX", Call::Sys(Val::N(32767))),
    ("TTY_NAME_MAX", Call::Sys(Val::N(32))),
    ("TZNAME_MAX", Call::Sys(Val::U)),
    ("UCHAR_MAX", Call::Sys(Val::N(255))),
    ("UINT_MAX", Call::Sys(Val::N(4294967295))),
    ("UIO_MAXIOV", Call::Sys(Val::N(1024))),
    ("ULONG_MAX", Call::Sys(Val::UlongMax)),
    ("USHRT_MAX", Call::Sys(Val::N(65535))),
    ("WORD_BIT", Call::Sys(Val::N(32))),
    ("_AVPHYS_PAGES", Call::Sys(Val::AvphysPages)),
    ("_NPROCESSORS_CONF", Call::Sys(Val::NprocConf)),
    ("NPROCESSORS_CONF", Call::Sys(Val::NprocConf)),
    ("_NPROCESSORS_ONLN", Call::Sys(Val::NprocOnln)),
    ("NPROCESSORS_ONLN", Call::Sys(Val::NprocOnln)),
    ("_PHYS_PAGES", Call::Sys(Val::PhysPages)),
    ("_POSIX_ARG_MAX", Call::Sys(Val::ArgMax)),
    ("_POSIX_ASYNCHRONOUS_IO", Call::Sys(Val::N(200809))),
    ("_POSIX_CHILD_MAX", Call::Sys(Val::ChildMax)),
    ("_POSIX_FSYNC", Call::Sys(Val::N(200809))),
    ("_POSIX_JOB_CONTROL", Call::Sys(Val::N(1))),
    ("_POSIX_MAPPED_FILES", Call::Sys(Val::N(200809))),
    ("_POSIX_MEMLOCK", Call::Sys(Val::N(200809))),
    ("_POSIX_MEMLOCK_RANGE", Call::Sys(Val::N(200809))),
    ("_POSIX_MEMORY_PROTECTION", Call::Sys(Val::N(200809))),
    ("_POSIX_MESSAGE_PASSING", Call::Sys(Val::N(200809))),
    ("_POSIX_NGROUPS_MAX", Call::Sys(Val::NgroupsMax)),
    ("_POSIX_OPEN_MAX", Call::Sys(Val::OpenMax)),
    ("_POSIX_PII", Call::Sys(Val::U)),
    ("_POSIX_PII_INTERNET", Call::Sys(Val::U)),
    ("_POSIX_PII_INTERNET_DGRAM", Call::Sys(Val::U)),
    ("_POSIX_PII_INTERNET_STREAM", Call::Sys(Val::U)),
    ("_POSIX_PII_OSI", Call::Sys(Val::U)),
    ("_POSIX_PII_OSI_CLTS", Call::Sys(Val::U)),
    ("_POSIX_PII_OSI_COTS", Call::Sys(Val::U)),
    ("_POSIX_PII_OSI_M", Call::Sys(Val::U)),
    ("_POSIX_PII_SOCKET", Call::Sys(Val::U)),
    ("_POSIX_PII_XTI", Call::Sys(Val::U)),
    ("_POSIX_POLL", Call::Sys(Val::U)),
    ("_POSIX_PRIORITIZED_IO", Call::Sys(Val::N(200809))),
    ("_POSIX_PRIORITY_SCHEDULING", Call::Sys(Val::N(200809))),
    ("_POSIX_REALTIME_SIGNALS", Call::Sys(Val::N(200809))),
    ("_POSIX_SAVED_IDS", Call::Sys(Val::N(1))),
    ("_POSIX_SELECT", Call::Sys(Val::U)),
    ("_POSIX_SEMAPHORES", Call::Sys(Val::N(200809))),
    ("_POSIX_SHARED_MEMORY_OBJECTS", Call::Sys(Val::N(200809))),
    ("_POSIX_SSIZE_MAX", Call::Sys(Val::N(32767))),
    ("_POSIX_STREAM_MAX", Call::Sys(Val::N(16))),
    ("_POSIX_SYNCHRONIZED_IO", Call::Sys(Val::N(200809))),
    ("_POSIX_THREADS", Call::Sys(Val::N(200809))),
    ("_POSIX_THREAD_ATTR_STACKADDR", Call::Sys(Val::N(200809))),
    ("_POSIX_THREAD_ATTR_STACKSIZE", Call::Sys(Val::N(200809))),
    (
        "_POSIX_THREAD_PRIORITY_SCHEDULING",
        Call::Sys(Val::N(200809)),
    ),
    ("_POSIX_THREAD_PRIO_INHERIT", Call::Sys(Val::N(200809))),
    ("_POSIX_THREAD_PRIO_PROTECT", Call::Sys(Val::N(200809))),
    ("_POSIX_THREAD_ROBUST_PRIO_INHERIT", Call::Sys(Val::U)),
    ("_POSIX_THREAD_ROBUST_PRIO_PROTECT", Call::Sys(Val::U)),
    ("_POSIX_THREAD_PROCESS_SHARED", Call::Sys(Val::N(200809))),
    ("_POSIX_THREAD_SAFE_FUNCTIONS", Call::Sys(Val::N(200809))),
    ("_POSIX_TIMERS", Call::Sys(Val::N(200809))),
    ("TIMER_MAX", Call::Sys(Val::U)),
    ("_POSIX_TZNAME_MAX", Call::Sys(Val::U)),
    ("_POSIX_VERSION", Call::Sys(Val::N(200809))),
    ("_T_IOV_MAX", Call::Sys(Val::U)),
    ("_XOPEN_CRYPT", Call::Sys(Val::U)),
    ("_XOPEN_ENH_I18N", Call::Sys(Val::N(1))),
    ("_XOPEN_LEGACY", Call::Sys(Val::N(1))),
    ("_XOPEN_REALTIME", Call::Sys(Val::N(1))),
    ("_XOPEN_REALTIME_THREADS", Call::Sys(Val::N(1))),
    ("_XOPEN_SHM", Call::Sys(Val::N(1))),
    ("_XOPEN_UNIX", Call::Sys(Val::N(1))),
    ("_XOPEN_VERSION", Call::Sys(Val::N(700))),
    ("_XOPEN_XCU_VERSION", Call::Sys(Val::N(4))),
    ("_XOPEN_XPG2", Call::Sys(Val::N(1))),
    ("_XOPEN_XPG3", Call::Sys(Val::N(1))),
    ("_XOPEN_XPG4", Call::Sys(Val::N(1))),
    ("BC_BASE_MAX", Call::Sys(Val::N(99))),
    ("BC_DIM_MAX", Call::Sys(Val::N(2048))),
    ("BC_SCALE_MAX", Call::Sys(Val::N(99))),
    ("BC_STRING_MAX", Call::Sys(Val::N(1000))),
    ("CHARCLASS_NAME_MAX", Call::Sys(Val::N(2048))),
    ("COLL_WEIGHTS_MAX", Call::Sys(Val::N(255))),
    ("EQUIV_CLASS_MAX", Call::Sys(Val::U)),
    ("EXPR_NEST_MAX", Call::Sys(Val::N(32))),
    ("LINE_MAX", Call::Sys(Val::N(2048))),
    ("POSIX2_BC_BASE_MAX", Call::Sys(Val::N(99))),
    ("POSIX2_BC_DIM_MAX", Call::Sys(Val::N(2048))),
    ("POSIX2_BC_SCALE_MAX", Call::Sys(Val::N(99))),
    ("POSIX2_BC_STRING_MAX", Call::Sys(Val::N(1000))),
    ("POSIX2_CHAR_TERM", Call::Sys(Val::N(200809))),
    ("POSIX2_COLL_WEIGHTS_MAX", Call::Sys(Val::N(255))),
    ("POSIX2_C_BIND", Call::Sys(Val::N(200809))),
    ("POSIX2_C_DEV", Call::Sys(Val::N(200809))),
    ("POSIX2_C_VERSION", Call::Sys(Val::N(200809))),
    ("POSIX2_EXPR_NEST_MAX", Call::Sys(Val::N(32))),
    ("POSIX2_FORT_DEV", Call::Sys(Val::U)),
    ("POSIX2_FORT_RUN", Call::Sys(Val::U)),
    ("_POSIX2_LINE_MAX", Call::Sys(Val::N(2048))),
    ("POSIX2_LINE_MAX", Call::Sys(Val::N(2048))),
    ("POSIX2_LOCALEDEF", Call::Sys(Val::N(200809))),
    ("POSIX2_RE_DUP_MAX", Call::Sys(Val::N(32767))),
    ("POSIX2_SW_DEV", Call::Sys(Val::N(200809))),
    ("POSIX2_UPE", Call::Sys(Val::U)),
    ("POSIX2_VERSION", Call::Sys(Val::N(200809))),
    ("RE_DUP_MAX", Call::Sys(Val::N(32767))),
    ("PATH", Call::Conf("/bin:/usr/bin")),
    ("CS_PATH", Call::Conf("/bin:/usr/bin")),
    ("LFS_CFLAGS", Call::Conf("")),
    ("LFS_LDFLAGS", Call::Conf("")),
    ("LFS_LIBS", Call::Conf("")),
    ("LFS_LINTFLAGS", Call::Conf("")),
    ("LFS64_CFLAGS", Call::Conf("-D_LARGEFILE64_SOURCE")),
    ("LFS64_LDFLAGS", Call::Conf("")),
    ("LFS64_LIBS", Call::Conf("")),
    ("LFS64_LINTFLAGS", Call::Conf("-D_LARGEFILE64_SOURCE")),
    ("_XBS5_WIDTH_RESTRICTED_ENVS", Call::Conf("XBS5_LP64_OFF64")),
    ("XBS5_WIDTH_RESTRICTED_ENVS", Call::Conf("XBS5_LP64_OFF64")),
    ("_XBS5_ILP32_OFF32", Call::Sys(Val::U)),
    ("XBS5_ILP32_OFF32_CFLAGS", Call::Conf("")),
    ("XBS5_ILP32_OFF32_LDFLAGS", Call::Conf("")),
    ("XBS5_ILP32_OFF32_LIBS", Call::Conf("")),
    ("XBS5_ILP32_OFF32_LINTFLAGS", Call::Conf("")),
    ("_XBS5_ILP32_OFFBIG", Call::Sys(Val::U)),
    ("XBS5_ILP32_OFFBIG_CFLAGS", Call::Conf("")),
    ("XBS5_ILP32_OFFBIG_LDFLAGS", Call::Conf("")),
    ("XBS5_ILP32_OFFBIG_LIBS", Call::Conf("")),
    ("XBS5_ILP32_OFFBIG_LINTFLAGS", Call::Conf("")),
    ("_XBS5_LP64_OFF64", Call::Sys(Val::N(1))),
    ("XBS5_LP64_OFF64_CFLAGS", Call::Conf("-m64")),
    ("XBS5_LP64_OFF64_LDFLAGS", Call::Conf("-m64")),
    ("XBS5_LP64_OFF64_LIBS", Call::Conf("")),
    ("XBS5_LP64_OFF64_LINTFLAGS", Call::Conf("")),
    ("_XBS5_LPBIG_OFFBIG", Call::Sys(Val::U)),
    ("XBS5_LPBIG_OFFBIG_CFLAGS", Call::Conf("")),
    ("XBS5_LPBIG_OFFBIG_LDFLAGS", Call::Conf("")),
    ("XBS5_LPBIG_OFFBIG_LIBS", Call::Conf("")),
    ("XBS5_LPBIG_OFFBIG_LINTFLAGS", Call::Conf("")),
    ("_POSIX_V6_ILP32_OFF32", Call::Sys(Val::U)),
    ("POSIX_V6_ILP32_OFF32_CFLAGS", Call::Conf("")),
    ("POSIX_V6_ILP32_OFF32_LDFLAGS", Call::Conf("")),
    ("POSIX_V6_ILP32_OFF32_LIBS", Call::Conf("")),
    ("POSIX_V6_ILP32_OFF32_LINTFLAGS", Call::Conf("")),
    (
        "_POSIX_V6_WIDTH_RESTRICTED_ENVS",
        Call::Conf("POSIX_V6_LP64_OFF64"),
    ),
    (
        "POSIX_V6_WIDTH_RESTRICTED_ENVS",
        Call::Conf("POSIX_V6_LP64_OFF64"),
    ),
    ("_POSIX_V6_ILP32_OFFBIG", Call::Sys(Val::U)),
    ("POSIX_V6_ILP32_OFFBIG_CFLAGS", Call::Conf("")),
    ("POSIX_V6_ILP32_OFFBIG_LDFLAGS", Call::Conf("")),
    ("POSIX_V6_ILP32_OFFBIG_LIBS", Call::Conf("")),
    ("POSIX_V6_ILP32_OFFBIG_LINTFLAGS", Call::Conf("")),
    ("_POSIX_V6_LP64_OFF64", Call::Sys(Val::N(1))),
    ("POSIX_V6_LP64_OFF64_CFLAGS", Call::Conf("-m64")),
    ("POSIX_V6_LP64_OFF64_LDFLAGS", Call::Conf("-m64")),
    ("POSIX_V6_LP64_OFF64_LIBS", Call::Conf("")),
    ("POSIX_V6_LP64_OFF64_LINTFLAGS", Call::Conf("")),
    ("_POSIX_V6_LPBIG_OFFBIG", Call::Sys(Val::U)),
    ("POSIX_V6_LPBIG_OFFBIG_CFLAGS", Call::Conf("")),
    ("POSIX_V6_LPBIG_OFFBIG_LDFLAGS", Call::Conf("")),
    ("POSIX_V6_LPBIG_OFFBIG_LIBS", Call::Conf("")),
    ("POSIX_V6_LPBIG_OFFBIG_LINTFLAGS", Call::Conf("")),
    ("_POSIX_V7_ILP32_OFF32", Call::Sys(Val::U)),
    ("POSIX_V7_ILP32_OFF32_CFLAGS", Call::Conf("")),
    ("POSIX_V7_ILP32_OFF32_LDFLAGS", Call::Conf("")),
    ("POSIX_V7_ILP32_OFF32_LIBS", Call::Conf("")),
    ("POSIX_V7_ILP32_OFF32_LINTFLAGS", Call::Conf("")),
    (
        "_POSIX_V7_WIDTH_RESTRICTED_ENVS",
        Call::Conf("POSIX_V7_LP64_OFF64"),
    ),
    (
        "POSIX_V7_WIDTH_RESTRICTED_ENVS",
        Call::Conf("POSIX_V7_LP64_OFF64"),
    ),
    ("_POSIX_V7_ILP32_OFFBIG", Call::Sys(Val::U)),
    ("POSIX_V7_ILP32_OFFBIG_CFLAGS", Call::Conf("")),
    ("POSIX_V7_ILP32_OFFBIG_LDFLAGS", Call::Conf("")),
    ("POSIX_V7_ILP32_OFFBIG_LIBS", Call::Conf("")),
    ("POSIX_V7_ILP32_OFFBIG_LINTFLAGS", Call::Conf("")),
    ("_POSIX_V7_LP64_OFF64", Call::Sys(Val::N(1))),
    ("POSIX_V7_LP64_OFF64_CFLAGS", Call::Conf("-m64")),
    ("POSIX_V7_LP64_OFF64_LDFLAGS", Call::Conf("-m64")),
    ("POSIX_V7_LP64_OFF64_LIBS", Call::Conf("")),
    ("POSIX_V7_LP64_OFF64_LINTFLAGS", Call::Conf("")),
    ("_POSIX_V7_LPBIG_OFFBIG", Call::Sys(Val::U)),
    ("POSIX_V7_LPBIG_OFFBIG_CFLAGS", Call::Conf("")),
    ("POSIX_V7_LPBIG_OFFBIG_LDFLAGS", Call::Conf("")),
    ("POSIX_V7_LPBIG_OFFBIG_LIBS", Call::Conf("")),
    ("POSIX_V7_LPBIG_OFFBIG_LINTFLAGS", Call::Conf("")),
    ("_POSIX_ADVISORY_INFO", Call::Sys(Val::N(200809))),
    ("_POSIX_BARRIERS", Call::Sys(Val::N(200809))),
    ("_POSIX_BASE", Call::Sys(Val::U)),
    ("_POSIX_C_LANG_SUPPORT", Call::Sys(Val::U)),
    ("_POSIX_C_LANG_SUPPORT_R", Call::Sys(Val::U)),
    ("_POSIX_CLOCK_SELECTION", Call::Sys(Val::N(200809))),
    ("_POSIX_CPUTIME", Call::Sys(Val::N(200809))),
    ("_POSIX_THREAD_CPUTIME", Call::Sys(Val::N(200809))),
    ("_POSIX_DEVICE_SPECIFIC", Call::Sys(Val::U)),
    ("_POSIX_DEVICE_SPECIFIC_R", Call::Sys(Val::U)),
    ("_POSIX_FD_MGMT", Call::Sys(Val::U)),
    ("_POSIX_FIFO", Call::Sys(Val::U)),
    ("_POSIX_PIPE", Call::Sys(Val::U)),
    ("_POSIX_FILE_ATTRIBUTES", Call::Sys(Val::U)),
    ("_POSIX_FILE_LOCKING", Call::Sys(Val::U)),
    ("_POSIX_FILE_SYSTEM", Call::Sys(Val::U)),
    ("_POSIX_MONOTONIC_CLOCK", Call::Sys(Val::N(200809))),
    ("_POSIX_MULTI_PROCESS", Call::Sys(Val::U)),
    ("_POSIX_SINGLE_PROCESS", Call::Sys(Val::U)),
    ("_POSIX_NETWORKING", Call::Sys(Val::U)),
    ("_POSIX_READER_WRITER_LOCKS", Call::Sys(Val::N(200809))),
    ("_POSIX_SPIN_LOCKS", Call::Sys(Val::N(200809))),
    ("_POSIX_REGEXP", Call::Sys(Val::N(1))),
    ("_REGEX_VERSION", Call::Sys(Val::U)),
    ("_POSIX_SHELL", Call::Sys(Val::N(1))),
    ("_POSIX_SIGNALS", Call::Sys(Val::U)),
    ("_POSIX_SPAWN", Call::Sys(Val::N(200809))),
    ("_POSIX_SPORADIC_SERVER", Call::Sys(Val::U)),
    ("_POSIX_THREAD_SPORADIC_SERVER", Call::Sys(Val::U)),
    ("_POSIX_SYSTEM_DATABASE", Call::Sys(Val::U)),
    ("_POSIX_SYSTEM_DATABASE_R", Call::Sys(Val::U)),
    ("_POSIX_TIMEOUTS", Call::Sys(Val::N(200809))),
    ("_POSIX_TYPED_MEMORY_OBJECTS", Call::Sys(Val::U)),
    ("_POSIX_USER_GROUPS", Call::Sys(Val::U)),
    ("_POSIX_USER_GROUPS_R", Call::Sys(Val::U)),
    ("POSIX2_PBS", Call::Sys(Val::U)),
    ("POSIX2_PBS_ACCOUNTING", Call::Sys(Val::U)),
    ("POSIX2_PBS_LOCATE", Call::Sys(Val::U)),
    ("POSIX2_PBS_TRACK", Call::Sys(Val::U)),
    ("POSIX2_PBS_MESSAGE", Call::Sys(Val::U)),
    ("SYMLOOP_MAX", Call::Sys(Val::U)),
    ("STREAM_MAX", Call::Sys(Val::N(16))),
    ("AIO_LISTIO_MAX", Call::Sys(Val::U)),
    ("AIO_MAX", Call::Sys(Val::U)),
    ("AIO_PRIO_DELTA_MAX", Call::Sys(Val::N(20))),
    ("DELAYTIMER_MAX", Call::Sys(Val::N(2147483647))),
    ("HOST_NAME_MAX", Call::Sys(Val::N(64))),
    ("LOGIN_NAME_MAX", Call::Sys(Val::N(256))),
    ("MQ_OPEN_MAX", Call::Sys(Val::U)),
    ("MQ_PRIO_MAX", Call::Sys(Val::N(32768))),
    ("_POSIX_DEVICE_IO", Call::Sys(Val::U)),
    ("_POSIX_TRACE", Call::Sys(Val::U)),
    ("_POSIX_TRACE_EVENT_FILTER", Call::Sys(Val::U)),
    ("_POSIX_TRACE_INHERIT", Call::Sys(Val::U)),
    ("_POSIX_TRACE_LOG", Call::Sys(Val::U)),
    ("RTSIG_MAX", Call::Sys(Val::N(32))),
    ("SEM_NSEMS_MAX", Call::Sys(Val::U)),
    ("SEM_VALUE_MAX", Call::Sys(Val::N(2147483647))),
    ("SIGQUEUE_MAX", Call::Sys(Val::SigqueueMax)),
    ("FILESIZEBITS", Call::Path(Pc::FileSizeBits)),
    ("POSIX_ALLOC_SIZE_MIN", Call::Path(Pc::AllocSizeMin)),
    ("POSIX_REC_INCR_XFER_SIZE", Call::Path(Pc::RecIncrXferSize)),
    ("POSIX_REC_MAX_XFER_SIZE", Call::Path(Pc::RecMaxXferSize)),
    ("POSIX_REC_MIN_XFER_SIZE", Call::Path(Pc::RecMinXferSize)),
    ("POSIX_REC_XFER_ALIGN", Call::Path(Pc::RecXferAlign)),
    ("SYMLINK_MAX", Call::Path(Pc::SymlinkMax)),
    ("GNU_LIBC_VERSION", Call::Conf("glibc 2.41")),
    ("GNU_LIBPTHREAD_VERSION", Call::Conf("NPTL 2.41")),
    ("POSIX2_SYMLINKS", Call::Path(Pc::Symlinks2)),
    ("LEVEL1_ICACHE_SIZE", Call::Sys(Val::N(32768))),
    ("LEVEL1_ICACHE_ASSOC", Call::Sys(Val::U)),
    ("LEVEL1_ICACHE_LINESIZE", Call::Sys(Val::N(64))),
    ("LEVEL1_DCACHE_SIZE", Call::Sys(Val::N(32768))),
    ("LEVEL1_DCACHE_ASSOC", Call::Sys(Val::N(8))),
    ("LEVEL1_DCACHE_LINESIZE", Call::Sys(Val::N(64))),
    ("LEVEL2_CACHE_SIZE", Call::Sys(Val::N(524288))),
    ("LEVEL2_CACHE_ASSOC", Call::Sys(Val::N(8))),
    ("LEVEL2_CACHE_LINESIZE", Call::Sys(Val::N(64))),
    ("LEVEL3_CACHE_SIZE", Call::Sys(Val::N(16777216))),
    ("LEVEL3_CACHE_ASSOC", Call::Sys(Val::N(16))),
    ("LEVEL3_CACHE_LINESIZE", Call::Sys(Val::N(64))),
    ("LEVEL4_CACHE_SIZE", Call::Sys(Val::N(0))),
    ("LEVEL4_CACHE_ASSOC", Call::Sys(Val::U)),
    ("LEVEL4_CACHE_LINESIZE", Call::Sys(Val::U)),
    ("IPV6", Call::Sys(Val::N(200809))),
    ("RAW_SOCKETS", Call::Sys(Val::N(200809))),
    ("_POSIX_IPV6", Call::Sys(Val::N(200809))),
    ("_POSIX_RAW_SOCKETS", Call::Sys(Val::N(200809))),
];

const HELP: &str = "Usage: getconf [-v SPEC] VAR
  or:  getconf [-v SPEC] PATH_VAR PATH

Get the configuration value for variable VAR, or for variable PATH_VAR
for path PATH.  If SPEC is given, give values for compilation
environment SPEC.

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const VERSION: &str = "getconf (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Roland McGrath.
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Resultado de um `pathconf`.
enum PathRes {
    Val(i64),
    /// `-1` sem errno: o limite não existe ("undefined").
    Undef,
    Err(Errno),
}

// ---- constantes de sistemas de arquivos (linux_fsinfo.h) ----

const EXT2_SUPER_MAGIC: u64 = 0xef53;
const F2FS_SUPER_MAGIC: u64 = 0xf2f5_2010;
const MINIX_SUPER_MAGIC: u64 = 0x137f;
const MINIX_SUPER_MAGIC2: u64 = 0x138f;
const MINIX2_SUPER_MAGIC: u64 = 0x2468;
const MINIX2_SUPER_MAGIC2: u64 = 0x2478;
const XENIX_SUPER_MAGIC: u64 = 0x012f_f7b4;
const SYSV4_SUPER_MAGIC: u64 = 0x012f_f7b5;
const SYSV2_SUPER_MAGIC: u64 = 0x012f_f7b6;
const COH_SUPER_MAGIC: u64 = 0x012f_f7b7;
const UFS_MAGIC: u64 = 0x0001_1954;
const UFS_CIGAM: u64 = 0x5419_0100;
const REISERFS_SUPER_MAGIC: u64 = 0x5265_4973;
const XFS_SUPER_MAGIC: u64 = 0x5846_5342;
const LUSTRE_SUPER_MAGIC: u64 = 0x0bd0_0bd0;
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const SMB_SUPER_MAGIC: u64 = 0x517b;
const NTFS_SUPER_MAGIC: u64 = 0x5346_544e;
const UDF_SUPER_MAGIC: u64 = 0x1501_3346;
const JFS_SUPER_MAGIC: u64 = 0x3153_464a;
const VXFS_SUPER_MAGIC: u64 = 0xa501_fcf5;
const CGROUP_SUPER_MAGIC: u64 = 0x27e0eb;
const MSDOS_SUPER_MAGIC: u64 = 0x4d44;
const JFFS_SUPER_MAGIC: u64 = 0x07c0;
const JFFS2_SUPER_MAGIC: u64 = 0x72b6;
const NCP_SUPER_MAGIC: u64 = 0x564c;
const ROMFS_SUPER_MAGIC: u64 = 0x7275;
const ADFS_SUPER_MAGIC: u64 = 0xadf5;
const BFS_MAGIC: u64 = 0x1bad_face;
const CRAMFS_MAGIC: u64 = 0x28cd_3d45;
const DEVPTS_SUPER_MAGIC: u64 = 0x1cd1;
const EFS_SUPER_MAGIC: u64 = 0x41_4a53;
const EFS_MAGIC: u64 = 0x07_2959;
const QNX4_SUPER_MAGIC: u64 = 0x002f;

/// `__statfs_link_max` para um `f_type` conhecido. No ext2/3/4 a glibc distingue o ext4 pelo sysfs;
/// aqui vale o valor do ext4.
fn link_max(fs_type: u64) -> i64 {
    match fs_type {
        EXT2_SUPER_MAGIC => 65000,
        F2FS_SUPER_MAGIC => 32000,
        MINIX_SUPER_MAGIC | MINIX_SUPER_MAGIC2 => 250,
        MINIX2_SUPER_MAGIC | MINIX2_SUPER_MAGIC2 => 65530,
        XENIX_SUPER_MAGIC | SYSV4_SUPER_MAGIC | SYSV2_SUPER_MAGIC => 126,
        COH_SUPER_MAGIC => 10000,
        UFS_MAGIC | UFS_CIGAM => 32000,
        REISERFS_SUPER_MAGIC => 64535,
        XFS_SUPER_MAGIC => 2_147_483_647,
        LUSTRE_SUPER_MAGIC => 65000,
        _ => 127,
    }
}

/// `__statfs_filesize_max`.
fn filesize_bits(fs_type: u64) -> i64 {
    match fs_type {
        F2FS_SUPER_MAGIC => 256,
        BTRFS_SUPER_MAGIC => 255,
        EXT2_SUPER_MAGIC | UFS_MAGIC | UFS_CIGAM | REISERFS_SUPER_MAGIC | XFS_SUPER_MAGIC
        | SMB_SUPER_MAGIC | NTFS_SUPER_MAGIC | UDF_SUPER_MAGIC | JFS_SUPER_MAGIC
        | VXFS_SUPER_MAGIC | CGROUP_SUPER_MAGIC | LUSTRE_SUPER_MAGIC => 64,
        MSDOS_SUPER_MAGIC | JFFS_SUPER_MAGIC | JFFS2_SUPER_MAGIC | NCP_SUPER_MAGIC
        | ROMFS_SUPER_MAGIC => 32,
        _ => 32,
    }
}

/// `__statfs_symlinks`: 0 nos sistemas sem link simbólico.
fn symlinks(fs_type: u64) -> i64 {
    match fs_type {
        ADFS_SUPER_MAGIC | BFS_MAGIC | CRAMFS_MAGIC | DEVPTS_SUPER_MAGIC | EFS_SUPER_MAGIC
        | EFS_MAGIC | MSDOS_SUPER_MAGIC | NTFS_SUPER_MAGIC | QNX4_SUPER_MAGIC
        | ROMFS_SUPER_MAGIC => 0,
        _ => 1,
    }
}

/// `pathconf (path, name)` do Linux: os nomes com tratamento de sistema de arquivos primeiro, o
/// restante como em `sysdeps/posix/pathconf.c`.
fn pathconf(pc: Pc, path: &[u8]) -> PathRes {
    let kernel = sys::current();
    match pc {
        Pc::LinkMax => match kernel.statfs(path) {
            Ok(s) => PathRes::Val(link_max(s.fs_type)),
            Err(e) => PathRes::Err(e),
        },
        Pc::FileSizeBits => match kernel.statfs(path) {
            Ok(s) => PathRes::Val(filesize_bits(s.fs_type)),
            Err(e) => PathRes::Err(e),
        },
        Pc::Symlinks2 => match kernel.statfs(path) {
            Ok(s) => PathRes::Val(symlinks(s.fs_type)),
            Err(e) => PathRes::Err(e),
        },
        Pc::ChownRestricted => match kernel.statfs(path) {
            Ok(_) => PathRes::Val(1),
            Err(e) => PathRes::Err(e),
        },
        _ => {
            if path.is_empty() {
                return PathRes::Err(Errno::ENOENT);
            }
            // `statvfs`: f_bsize e f_frsize (este cai no f_bsize se vier zero) e f_namemax.
            let vfs = |f: &dyn Fn(&sysabi::StatFs) -> i64| match kernel.statfs(path) {
                Ok(s) => PathRes::Val(f(&s)),
                Err(e) => PathRes::Err(e),
            };
            let frsize = |s: &sysabi::StatFs| -> i64 {
                if s.frsize != 0 {
                    s.frsize as i64
                } else {
                    s.bsize as i64
                }
            };
            match pc {
                Pc::MaxCanon | Pc::MaxInput => PathRes::Val(255),
                Pc::NameMax => vfs(&|s| s.namelen as i64),
                Pc::PathMax | Pc::PipeBuf => PathRes::Val(4096),
                Pc::NoTrunc => PathRes::Val(1),
                Pc::Vdisable => PathRes::Val(0),
                Pc::AsyncIo => {
                    match kernel.fstatat(sysabi::Fd::CWD, path, sysabi::AtFlags::empty()) {
                        Err(e) => PathRes::Err(e),
                        Ok(st) => match st.file_type() {
                            FileType::Regular | FileType::BlockDevice => PathRes::Val(1),
                            _ => PathRes::Undef,
                        },
                    }
                }
                Pc::RecMinXferSize => vfs(&|s| s.bsize as i64),
                Pc::RecXferAlign | Pc::AllocSizeMin => vfs(&frsize),
                Pc::SockMaxbuf
                | Pc::PrioIo
                | Pc::SyncIo
                | Pc::RecIncrXferSize
                | Pc::RecMaxXferSize
                | Pc::SymlinkMax => PathRes::Undef,
                // Os quatro com tratamento próprio já saíram acima.
                Pc::LinkMax | Pc::FileSizeBits | Pc::Symlinks2 | Pc::ChownRestricted => {
                    PathRes::Undef
                }
            }
        }
    }
}

/// O valor `meminfo` em kB (`MemTotal`, `MemFree`), ou 0 sem o arquivo.
fn meminfo_kib(key: &str) -> i64 {
    let Ok(data) = sys::read_file(b"/proc/meminfo") else {
        return 0;
    };
    for line in data.split(|b| *b == b'\n') {
        let Some(rest) = line.strip_prefix(key.as_bytes()) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(b":") else {
            continue;
        };
        let text = String::from_utf8_lossy(rest);
        if let Some(n) = text
            .split_whitespace()
            .next()
            .and_then(|t| t.parse::<i64>().ok())
        {
            return n;
        }
    }
    0
}

/// Quantidade de CPUs de uma lista como `0-3,8` (o `read_sysfs_file` da glibc); 0 se ilegível.
fn cpu_list_count(path: &[u8]) -> i64 {
    let Ok(data) = sys::read_file(path) else {
        return 0;
    };
    let text = String::from_utf8_lossy(&data);
    let line = text.lines().next().unwrap_or("");
    let mut total = 0i64;
    for part in line.split(',') {
        let mut it = part.splitn(2, '-');
        let Some(a) = it.next().and_then(|t| t.trim().parse::<i64>().ok()) else {
            return 0;
        };
        let b = match it.next() {
            Some(t) => match t.trim().parse::<i64>() {
                Ok(b) => b,
                Err(_) => return 0,
            },
            None => a,
        };
        if b >= a {
            total += b - a + 1;
        }
    }
    total
}

/// `__get_nprocs_fallback`: `/proc/stat`, depois a máscara de afinidade, depois 2.
fn nprocs_fallback() -> i64 {
    if let Ok(data) = sys::read_file(b"/proc/stat") {
        let n = data
            .split(|b| *b == b'\n')
            .filter(|l| l.starts_with(b"cpu") && l.get(3).is_some_and(|c| c.is_ascii_digit()))
            .count() as i64;
        if n != 0 {
            return n;
        }
    }
    let n = sys::current().sched_getaffinity().len() as i64;
    if n != 0 { n } else { 2 }
}

fn sysconf(v: Val) -> i64 {
    let kernel = sys::current();
    match v {
        Val::N(n) => n,
        Val::U => -1,
        Val::UlongMax => -1,
        Val::ArgMax => {
            // MAX (131072, limite de pilha / 4), no máximo 6 MiB.
            const LEGACY: u64 = 131_072;
            const MAXIMUM: u64 = 6 * 1024 * 1024;
            match kernel.getrlimit(Resource::Stack) {
                Ok(r) => (LEGACY.max(r.cur / 4)).min(MAXIMUM) as i64,
                Err(_) => LEGACY as i64,
            }
        }
        Val::ChildMax => match kernel.getrlimit(Resource::Nproc) {
            Ok(r) if r.cur != RLIM_INFINITY => r.cur as i64,
            _ => -1,
        },
        Val::OpenMax => match kernel.getrlimit(Resource::Nofile) {
            // `getdtablesize` devolve o limite em `int`.
            Ok(r) => i64::from(r.cur as u32 as i32),
            Err(_) => 1024,
        },
        Val::NgroupsMax => {
            let from_proc = sys::read_file(b"/proc/sys/kernel/ngroups_max")
                .ok()
                .and_then(|d| {
                    let t = String::from_utf8_lossy(&d).into_owned();
                    t.trim_end_matches('\n').parse::<i64>().ok()
                });
            from_proc.unwrap_or(65536)
        }
        Val::SigqueueMax => match kernel.getrlimit(Resource::Sigpending) {
            Ok(r) => r.cur as i64,
            Err(_) => -1,
        },
        Val::NprocConf => {
            let n = cpu_list_count(b"/sys/devices/system/cpu/possible");
            if n != 0 { n } else { nprocs_fallback() }
        }
        Val::NprocOnln => {
            let n = cpu_list_count(b"/sys/devices/system/cpu/online");
            if n != 0 { n } else { nprocs_fallback() }
        }
        Val::PhysPages => meminfo_kib("MemTotal") * 1024 / 4096,
        Val::AvphysPages => meminfo_kib("MemFree") * 1024 / 4096,
    }
}

/// O programa chamado pelo `usage` (o `__progname`).
fn usage(argv0: &str) -> i32 {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    io::eprint(format!(
        "Usage: {name} [-v specification] variable_name [pathname]\n       {name} -a [pathname]\n"
    ));
    2
}

/// `error (status, errno, ...)`: `getconf: mensagem[: erro]` e sai com o código.
fn fail(argv0: &str, status: i32, msg: &str, errno: Option<Errno>) -> i32 {
    match errno {
        Some(e) => io::eprint(format!("{argv0}: {msg}: {}\n", e.message())),
        None => io::eprint(format!("{argv0}: {msg}\n")),
    }
    status
}

fn print_all(path: &[u8]) -> i32 {
    let mut out = io::stdout();
    for (name, call) in VARS {
        let mut line = format!("{name:<35}").into_bytes();
        match call {
            Call::Path(pc) => {
                if let PathRes::Val(v) = pathconf(*pc, path) {
                    line.extend_from_slice(v.to_string().as_bytes());
                }
            }
            Call::Sys(Val::UlongMax) => line.extend_from_slice(u64::MAX.to_string().as_bytes()),
            Call::Sys(v) => {
                let value = sysconf(*v);
                if value != -1 {
                    line.extend_from_slice(value.to_string().as_bytes());
                }
            }
            Call::Conf(s) => line.extend_from_slice(s.as_bytes()),
        }
        line.push(b'\n');
        let _ = out.write_all(&line);
    }
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut out = io::stdout();
    // Os argumentos depois do nome do programa; `rest[0]` é o `argv[1]` do C.
    let mut rest: &[Vec<u8>] = &argv[1..];

    if rest.first().is_some_and(|a| a == b"--version") {
        let _ = out.write_all(VERSION.as_bytes());
        return 0;
    }
    if rest.first().is_some_and(|a| a == b"--help") {
        let _ = out.write_all(HELP.as_bytes());
        return 0;
    }
    // `-v SPEC` ou `-vSPEC`: a especificação é aceita e ignorada (todos os ambientes são conhecidos).
    if let Some(first) = rest.first()
        && first.starts_with(b"-v")
    {
        if first.len() == 2 {
            if rest.len() < 2 {
                return usage(&argv0);
            }
            rest = &rest[2..];
        } else {
            rest = &rest[1..];
        }
    }
    // Daqui em diante `rest` começa onde o C usa `argv[1]`.
    if rest.first().is_some_and(|a| a == b"-a") {
        return match rest.len() {
            1 => print_all(b"/"),
            2 => print_all(&rest[1]),
            _ => usage(&argv0),
        };
    }
    let mut ai = 0usize;
    if rest.first().is_some_and(|a| a == b"--") {
        ai = 1;
    }
    let remaining = rest.len() - ai;
    if !(1..=2).contains(&remaining) {
        return usage(&argv0);
    }
    let var = &rest[ai];
    for (name, call) in VARS {
        let hit = name.as_bytes() == var.as_slice()
            || (name.starts_with("_POSIX_") && name.as_bytes()[7..] == var[..]);
        if !hit {
            continue;
        }
        return match call {
            Call::Path(pc) => {
                if remaining < 2 {
                    return usage(&argv0);
                }
                let path = &rest[ai + 1];
                match pathconf(*pc, path) {
                    PathRes::Val(v) => {
                        let _ = writeln!(out, "{v}");
                        0
                    }
                    PathRes::Undef => {
                        let _ = out.write_all(b"undefined\n");
                        0
                    }
                    PathRes::Err(e) => fail(
                        &argv0,
                        3,
                        &format!("pathconf: {}", io::lossy(path)),
                        Some(e),
                    ),
                }
            }
            Call::Sys(v) => {
                if remaining > 1 {
                    return usage(&argv0);
                }
                if let Val::UlongMax = v {
                    let _ = writeln!(out, "{}", u64::MAX);
                    return 0;
                }
                let value = sysconf(*v);
                if value == -1 {
                    let _ = out.write_all(b"undefined\n");
                } else {
                    let _ = writeln!(out, "{value}");
                }
                0
            }
            Call::Conf(s) => {
                if remaining > 1 {
                    return usage(&argv0);
                }
                let _ = writeln!(out, "{s}");
                0
            }
        };
    }
    fail(
        &argv0,
        2,
        &format!("Unrecognized variable `{}'", io::lossy(var)),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new().programs([Program::bin("getconf", main)])
    }

    #[test]
    fn table_has_every_variable_once_in_order() {
        assert_eq!(VARS.len(), 322);
        assert_eq!(VARS[0].0, "LINK_MAX");
        assert_eq!(VARS[VARS.len() - 1].0, "_POSIX_RAW_SOCKETS");
    }

    #[test]
    fn fixed_values_and_errors() {
        let k = kit();
        assert_eq!(k.run(&["getconf", "LONG_BIT"], b"").stdout_str(), "64\n");
        assert_eq!(k.run(&["getconf", "PAGESIZE"], b"").stdout_str(), "4096\n");
        assert_eq!(
            k.run(&["getconf", "ULONG_MAX"], b"").stdout_str(),
            "18446744073709551615\n"
        );
        assert_eq!(
            k.run(&["getconf", "GNU_LIBC_VERSION"], b"").stdout_str(),
            "glibc 2.41\n"
        );
        assert_eq!(
            k.run(&["getconf", "_POSIX_V7_LP64_OFF64"], b"")
                .stdout_str(),
            "1\n"
        );
        assert_eq!(
            k.run(&["getconf", "POSIX_V7_LP64_OFF64"], b"").stdout_str(),
            "1\n"
        );
        assert_eq!(
            k.run(&["getconf", "TZNAME_MAX"], b"").stdout_str(),
            "undefined\n"
        );
        let r = k.run(&["getconf", "FOO"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.code()),
            ("getconf: Unrecognized variable `FOO'\n", 2)
        );
        let r = k.run(&["getconf"], b"");
        assert_eq!(r.code(), 2);
        let r = k.run(&["getconf", "PATH_MAX"], b"");
        assert_eq!(r.code(), 2);
    }
}
