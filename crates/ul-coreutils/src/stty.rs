//! `stty` do GNU coreutils 9.7: porte à mão do `src/stty.c` sobre as chamadas de terminal do
//! `sysabi` (`tcgetattr`, `tcsetattr`, `tcgetwinsize`, `tcsetwinsize`). O uutils não serve de base:
//! o `stty` dele só cobre uma parte das opções e escreve outra saída.
//!
//! O que o programa original faz, na ordem:
//!
//! 1. Primeira passada tipo `getopt_long("agF:")` com `opterr = 0`: só `-a`/`--all`, `-g`/`--save`,
//!    `-F`/`--file` (e `--help`, `--version`) são consumidos; qualquer outra coisa (inclusive as
//!    configurações `-parenb`, `rows`, `5`) fica pra segunda passada e desliga o "sem argumentos".
//! 2. Combinações inválidas (`-a` com `-g`, saída com configurações, dois `-F`) morrem sem uso.
//! 3. Com `-F`, o dispositivo é aberto (sem bloquear); sem ele vale a entrada padrão. O
//!    `tcgetattr` vem **antes** de qualquer configuração ser interpretada, então fora de um terminal
//!    todo comando com configuração falha com `Inappropriate ioctl for device`.
//! 4. Sem configurações (ou com `-a`/`-g`) mostra o estado; senão aplica as configurações na ordem,
//!    manda o `tcsetattr` (`TCSADRAIN`, ou `TCSANOW` com `-drain`) e confere o que ficou.
//!
//! Limites do porte: a velocidade de entrada separada (`ispeed`) vai no campo `CIBAUD` do `c_cflag`
//! como a glibc faz, mas não há o bit `IBAUD0`; velocidades fora da tabela (`BOTHER`) aparecem como 0.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;

use sysabi::termios as t;
use sysabi::{Ctx, Errno, Fd, OFlags, SetAttrWhen, Syscalls, Termios, Winsize};

/// Texto do `--help`.
const HELP: &str = concat!(
    "Usage: stty [-F DEVICE | --file=DEVICE] [SETTING]...\n",
    "  or:  stty [-F DEVICE | --file=DEVICE] [-a|--all]\n",
    "  or:  stty [-F DEVICE | --file=DEVICE] [-g|--save]\n",
    "Print or change terminal characteristics.\n",
    "\n",
    "  -a, --all          print all current settings in human-readable form\n",
    "  -g, --save         print all current settings in a stty-readable form\n",
    "  -F, --file=DEVICE  open and use the specified DEVICE instead of stdin\n",
    "      --help        display this help and exit\n",
    "      --version     output version information and exit\n",
    "\n",
    "Optional - before SETTING indicates negation.  An * marks non-POSIX\n",
    "settings.  The underlying system defines which settings are available.\n",
    "\n",
    "Special characters:\n",
    " * discard CHAR  CHAR will toggle discarding of output\n",
    "   eof CHAR      CHAR will send an end of file (terminate the input)\n",
    "   eol CHAR      CHAR will end the line\n",
    " * eol2 CHAR     alternate CHAR for ending the line\n",
    "   erase CHAR    CHAR will erase the last character typed\n",
    "   intr CHAR     CHAR will send an interrupt signal\n",
    "   kill CHAR     CHAR will erase the current line\n",
    " * lnext CHAR    CHAR will enter the next character quoted\n",
    "   quit CHAR     CHAR will send a quit signal\n",
    " * rprnt CHAR    CHAR will redraw the current line\n",
    "   start CHAR    CHAR will restart output after stopping it\n",
    "   stop CHAR     CHAR will stop the output\n",
    "   susp CHAR     CHAR will send a terminal stop signal\n",
    " * swtch CHAR    CHAR will switch to a different shell layer\n",
    " * werase CHAR   CHAR will erase the last word typed\n",
    "\n",
    "Special settings:\n",
    "   N             set the input and output speeds to N bauds\n",
    " * cols N        tell the kernel that the terminal has N columns\n",
    " * columns N     same as cols N\n",
    "   [-]drain      wait for transmission before applying settings (on by default)\n",
    "   ispeed N      set the input speed to N\n",
    " * line N        use line discipline N\n",
    "   min N         with -icanon, set N characters minimum for a completed read\n",
    "   ospeed N      set the output speed to N\n",
    " * rows N        tell the kernel that the terminal has N rows\n",
    " * size          print the number of rows and columns that\n",
    "                   the kernel thinks the terminal has\n",
    "   speed         print the terminal speed\n",
    "   time N        with -icanon, set read timeout of N tenths of a second\n",
    "\n",
    "Control settings:\n",
    "   [-]clocal     disable modem control signals\n",
    "   [-]cread      allow input to be received\n",
    " * [-]crtscts    enable RTS/CTS handshaking\n",
    "   csN           set character size to N bits, N in [5..8]\n",
    "   [-]cstopb     use two stop bits per character (one with '-')\n",
    "   [-]hup        send a hangup signal when the last process closes\n",
    "                   the tty\n",
    "   [-]hupcl      same as [-]hup\n",
    "   [-]parenb     generate parity bit in output and expect parity bit in input\n",
    "   [-]parodd     set odd parity (or even parity with '-')\n",
    " * [-]cmspar     use \"stick\" (mark/space) parity\n",
    "\n",
    "Input settings:\n",
    "   [-]brkint     breaks cause an interrupt signal\n",
    "   [-]icrnl      translate carriage return to newline\n",
    "   [-]ignbrk     ignore break characters\n",
    "   [-]igncr      ignore carriage return\n",
    "   [-]ignpar     ignore characters with parity errors\n",
    " * [-]imaxbel    beep and do not flush a full input buffer on a character\n",
    "   [-]inlcr      translate newline to carriage return\n",
    "   [-]inpck      enable input parity checking\n",
    "   [-]istrip     clear high (8th) bit of input characters\n",
    " * [-]iutf8      assume input characters are UTF-8 encoded\n",
    " * [-]iuclc      translate uppercase characters to lowercase\n",
    " * [-]ixany      let any character restart output, not only start character\n",
    "   [-]ixoff      enable sending of start/stop characters\n",
    "   [-]ixon       enable XON/XOFF flow control\n",
    "   [-]parmrk     mark parity errors (with a 255-0-character sequence)\n",
    "   [-]tandem     same as [-]ixoff\n",
    "\n",
    "Output settings:\n",
    " * bsN           backspace delay style, N in [0..1]\n",
    " * crN           carriage return delay style, N in [0..3]\n",
    " * ffN           form feed delay style, N in [0..1]\n",
    " * nlN           newline delay style, N in [0..1]\n",
    " * [-]ocrnl      translate carriage return to newline\n",
    " * [-]ofdel      use delete characters for fill instead of null characters\n",
    " * [-]ofill      use fill (padding) characters instead of timing for delays\n",
    " * [-]olcuc      translate lowercase characters to uppercase\n",
    " * [-]onlcr      translate newline to carriage return-newline\n",
    " * [-]onlret     newline performs a carriage return\n",
    " * [-]onocr      do not print carriage returns in the first column\n",
    "   [-]opost      postprocess output\n",
    " * tabN          horizontal tab delay style, N in [0..3]\n",
    " * tabs          same as tab0\n",
    " * -tabs         same as tab3\n",
    " * vtN           vertical tab delay style, N in [0..1]\n",
    "\n",
    "Local settings:\n",
    "   [-]crterase   echo erase characters as backspace-space-backspace\n",
    " * crtkill       kill all line by obeying the echoprt and echoe settings\n",
    " * -crtkill      kill all line by obeying the echoctl and echok settings\n",
    " * [-]ctlecho    echo control characters in hat notation ('^c')\n",
    "   [-]echo       echo input characters\n",
    " * [-]echoctl    same as [-]ctlecho\n",
    "   [-]echoe      same as [-]crterase\n",
    "   [-]echok      echo newline after killing\n",
    " * [-]echoke     same as [-]crtkill\n",
    "   [-]echonl     echo newline even if not echoing other characters\n",
    " * [-]echoprt    echo erased characters backward, between '\\' and '/'\n",
    " * [-]extproc    enable \"LINEMODE\"; useful with high latency links\n",
    " * [-]flusho     discard output\n",
    "   [-]icanon     enable special characters: erase, kill, werase, rprnt\n",
    "   [-]iexten     enable non-POSIX special characters\n",
    "   [-]isig       enable interrupt, quit, and suspend special characters\n",
    "   [-]noflsh     disable flushing after interrupt and quit special characters\n",
    " * [-]prterase   same as [-]echoprt\n",
    " * [-]tostop     stop background jobs that try to write to the terminal\n",
    " * [-]xcase      with icanon, escape with '\\' for uppercase characters\n",
    "\n",
    "Combination settings:\n",
    " * [-]LCASE      same as [-]lcase\n",
    "   cbreak        same as -icanon\n",
    "   -cbreak       same as icanon\n",
    "   cooked        same as brkint ignpar istrip icrnl ixon opost isig\n",
    "                 icanon, eof and eol characters to their default values\n",
    "   -cooked       same as raw\n",
    "   crt           same as echoe echoctl echoke\n",
    "   dec           same as echoe echoctl echoke -ixany intr ^c erase 0177\n",
    "                 kill ^u\n",
    " * [-]decctlq    same as [-]ixany\n",
    "   ek            erase and kill characters to their default values\n",
    "   evenp         same as parenb -parodd cs7\n",
    "   -evenp        same as -parenb cs8\n",
    " * [-]lcase      same as xcase iuclc olcuc\n",
    "   litout        same as -parenb -istrip -opost cs8\n",
    "   -litout       same as parenb istrip opost cs7\n",
    "   nl            same as -icrnl -onlcr\n",
    "   -nl           same as icrnl -inlcr -igncr onlcr -ocrnl -onlret\n",
    "   oddp          same as parenb parodd cs7\n",
    "   -oddp         same as -parenb cs8\n",
    "   [-]parity     same as [-]evenp\n",
    "   pass8         same as -parenb -istrip cs8\n",
    "   -pass8        same as parenb istrip cs7\n",
    "   raw           same as -ignbrk -brkint -ignpar -parmrk -inpck -istrip\n",
    "                 -inlcr -igncr -icrnl -ixon -ixoff -icanon -opost\n",
    "                 -isig -iuclc -ixany -imaxbel -xcase min 1 time 0\n",
    "   -raw          same as cooked\n",
    "   sane          same as cread -ignbrk brkint -inlcr -igncr icrnl\n",
    "                 icrnl -ixoff -iuclc -ixany imaxbel opost -olcuc -ocrnl\n",
    "                 onlcr -onocr -onlret -ofill -ofdel nl0 cr0 tab0 bs0 vt0\n",
    "                 ff0 isig icanon iexten echo echoe echok -echonl\n",
    "                 -noflsh -xcase -tostop -echoprt echoctl echoke -extproc\n",
    "                 -flusho, all special characters to their default values\n",
    "\n",
    "Handle the tty line connected to standard input.  Without arguments,\n",
    "prints baud rate, line discipline, and deviations from stty sane.  In\n",
    "settings, CHAR is taken literally, or coded as in ^c, 0x37, 0177 or\n",
    "M-^c; ^- or undef disables special characters.\n",
    "\n",
    "GNU coreutils online help: <https://www.gnu.org/software/coreutils/>\n",
    "Report any translation bugs to <https://translationproject.org/team/>\n",
    "Full documentation <https://www.gnu.org/software/coreutils/stty>\n",
    "or available locally via: info '(coreutils) stty invocation'\n",
);

/// Texto do `--version`.
const VERSION: &str = concat!(
    "stty (GNU coreutils) 9.7\n",
    "Copyright (C) 2025 Free Software Foundation, Inc.\n",
    "License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.\n",
    "This is free software: you are free to change and redistribute it.\n",
    "There is NO WARRANTY, to the extent permitted by law.\n",
    "\n",
    "Written by David MacKenzie.\n",
);

/// Número de entradas do `c_cc` da glibc (`NCCS`), que é o que o `-g` escreve; o kernel só tem 19, o
/// resto vale zero.
const GLIBC_NCCS: usize = 32;

// ---------------------------------------------------------------------------------------------
// Tabelas

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Control,
    Input,
    Output,
    Local,
    Combination,
}

const CTL: Kind = Kind::Control;
const INP: Kind = Kind::Input;
const OUT: Kind = Kind::Output;
const LOC: Kind = Kind::Local;
const COMB: Kind = Kind::Combination;

/// A configuração tem forma negada (`-nome`).
const REV: u8 = 1;
/// `sane` liga a configuração.
const SANE_SET: u8 = 2;
/// `sane` desliga a configuração.
const SANE_UNSET: u8 = 4;
/// Não aparece na saída (apelidos e combinações).
const OMIT: u8 = 8;

struct ModeInfo {
    name: &'static str,
    kind: Kind,
    flags: u8,
    bits: u32,
    mask: u32,
}

const fn mi(name: &'static str, kind: Kind, flags: u8, bits: u32, mask: u32) -> ModeInfo {
    ModeInfo { name, kind, flags, bits, mask }
}

/// O `mode_info` do stty.c, na mesma ordem (a ordem define a saída do `-a`).
const MODES: &[ModeInfo] = &[
    mi("parenb", CTL, REV, t::PARENB, 0),
    mi("parodd", CTL, REV, t::PARODD, 0),
    mi("cmspar", CTL, REV, t::CMSPAR, 0),
    mi("cs5", CTL, 0, t::CS5, t::CSIZE),
    mi("cs6", CTL, 0, t::CS6, t::CSIZE),
    mi("cs7", CTL, 0, t::CS7, t::CSIZE),
    mi("cs8", CTL, 0, t::CS8, t::CSIZE),
    mi("hupcl", CTL, REV, t::HUPCL, 0),
    mi("hup", CTL, REV | OMIT, t::HUPCL, 0),
    mi("cstopb", CTL, REV, t::CSTOPB, 0),
    mi("cread", CTL, SANE_SET | REV, t::CREAD, 0),
    mi("clocal", CTL, REV, t::CLOCAL, 0),
    mi("crtscts", CTL, REV, t::CRTSCTS, 0),
    mi("ignbrk", INP, SANE_UNSET | REV, t::IGNBRK, 0),
    mi("brkint", INP, SANE_SET | REV, t::BRKINT, 0),
    mi("ignpar", INP, REV, t::IGNPAR, 0),
    mi("parmrk", INP, REV, t::PARMRK, 0),
    mi("inpck", INP, REV, t::INPCK, 0),
    mi("istrip", INP, REV, t::ISTRIP, 0),
    mi("inlcr", INP, SANE_UNSET | REV, t::INLCR, 0),
    mi("igncr", INP, SANE_UNSET | REV, t::IGNCR, 0),
    mi("icrnl", INP, SANE_SET | REV, t::ICRNL, 0),
    mi("ixon", INP, REV, t::IXON, 0),
    mi("ixoff", INP, SANE_UNSET | REV, t::IXOFF, 0),
    mi("tandem", INP, REV | OMIT, t::IXOFF, 0),
    mi("iuclc", INP, SANE_UNSET | REV, t::IUCLC, 0),
    mi("ixany", INP, SANE_UNSET | REV, t::IXANY, 0),
    mi("imaxbel", INP, SANE_SET | REV, t::IMAXBEL, 0),
    mi("iutf8", INP, SANE_UNSET | REV, t::IUTF8, 0),
    mi("opost", OUT, REV, t::OPOST, 0),
    mi("olcuc", OUT, SANE_UNSET | REV, t::OLCUC, 0),
    mi("ocrnl", OUT, SANE_UNSET | REV, t::OCRNL, 0),
    mi("onlcr", OUT, SANE_SET | REV, t::ONLCR, 0),
    mi("onocr", OUT, SANE_UNSET | REV, t::ONOCR, 0),
    mi("onlret", OUT, SANE_UNSET | REV, t::ONLRET, 0),
    mi("ofill", OUT, SANE_UNSET | REV, t::OFILL, 0),
    mi("ofdel", OUT, SANE_UNSET | REV, t::OFDEL, 0),
    mi("nl1", OUT, SANE_UNSET, t::NL1, t::NLDLY),
    mi("nl0", OUT, SANE_SET, t::NL0, t::NLDLY),
    mi("cr3", OUT, SANE_UNSET, t::CR3, t::CRDLY),
    mi("cr2", OUT, SANE_UNSET, t::CR2, t::CRDLY),
    mi("cr1", OUT, SANE_UNSET, t::CR1, t::CRDLY),
    mi("cr0", OUT, SANE_SET, t::CR0, t::CRDLY),
    mi("tab3", OUT, SANE_UNSET, t::TAB3, t::TABDLY),
    mi("tab2", OUT, SANE_UNSET, t::TAB2, t::TABDLY),
    mi("tab1", OUT, SANE_UNSET, t::TAB1, t::TABDLY),
    mi("tab0", OUT, SANE_SET, t::TAB0, t::TABDLY),
    mi("bs1", OUT, SANE_UNSET, t::BS1, t::BSDLY),
    mi("bs0", OUT, SANE_SET, t::BS0, t::BSDLY),
    mi("vt1", OUT, SANE_UNSET, t::VT1, t::VTDLY),
    mi("vt0", OUT, SANE_SET, t::VT0, t::VTDLY),
    mi("ff1", OUT, SANE_UNSET, t::FF1, t::FFDLY),
    mi("ff0", OUT, SANE_SET, t::FF0, t::FFDLY),
    mi("isig", LOC, SANE_SET | REV, t::ISIG, 0),
    mi("icanon", LOC, SANE_SET | REV, t::ICANON, 0),
    mi("iexten", LOC, SANE_SET | REV, t::IEXTEN, 0),
    mi("echo", LOC, SANE_SET | REV, t::ECHO, 0),
    mi("echoe", LOC, SANE_SET | REV, t::ECHOE, 0),
    mi("crterase", LOC, REV | OMIT, t::ECHOE, 0),
    mi("echok", LOC, SANE_SET | REV, t::ECHOK, 0),
    mi("echonl", LOC, SANE_UNSET | REV, t::ECHONL, 0),
    mi("noflsh", LOC, SANE_UNSET | REV, t::NOFLSH, 0),
    mi("xcase", LOC, SANE_UNSET | REV, t::XCASE, 0),
    mi("tostop", LOC, SANE_UNSET | REV, t::TOSTOP, 0),
    mi("echoprt", LOC, SANE_UNSET | REV, t::ECHOPRT, 0),
    mi("prterase", LOC, REV | OMIT, t::ECHOPRT, 0),
    mi("echoctl", LOC, SANE_SET | REV, t::ECHOCTL, 0),
    mi("ctlecho", LOC, REV | OMIT, t::ECHOCTL, 0),
    mi("echoke", LOC, SANE_SET | REV, t::ECHOKE, 0),
    mi("crtkill", LOC, REV | OMIT, t::ECHOKE, 0),
    mi("flusho", LOC, SANE_UNSET | REV, t::FLUSHO, 0),
    mi("extproc", LOC, SANE_UNSET | REV, t::EXTPROC, 0),
    mi("drain", COMB, REV | OMIT, 0, 0),
    mi("evenp", COMB, REV | OMIT, 0, 0),
    mi("parity", COMB, REV | OMIT, 0, 0),
    mi("oddp", COMB, REV | OMIT, 0, 0),
    mi("nl", COMB, REV | OMIT, 0, 0),
    mi("ek", COMB, OMIT, 0, 0),
    mi("sane", COMB, OMIT, 0, 0),
    mi("cooked", COMB, REV | OMIT, 0, 0),
    mi("raw", COMB, REV | OMIT, 0, 0),
    mi("pass8", COMB, REV | OMIT, 0, 0),
    mi("litout", COMB, REV | OMIT, 0, 0),
    mi("cbreak", COMB, REV | OMIT, 0, 0),
    mi("decctlq", COMB, REV | OMIT, 0, 0),
    mi("tabs", COMB, REV | OMIT, 0, 0),
    mi("lcase", COMB, REV | OMIT, 0, 0),
    mi("LCASE", COMB, REV | OMIT, 0, 0),
    mi("crt", COMB, OMIT, 0, 0),
    mi("dec", COMB, OMIT, 0, 0),
];

struct ControlInfo {
    name: &'static str,
    sane: u8,
    offset: usize,
}

const fn ci(name: &'static str, sane: u8, offset: usize) -> ControlInfo {
    ControlInfo { name, sane, offset }
}

/// O `control_info` do stty.c: os caracteres especiais e, no fim, `min` e `time` (que a exibição
/// trata à parte).
const CONTROLS: &[ControlInfo] = &[
    ci("intr", 0o03, t::VINTR),
    ci("quit", 0o34, t::VQUIT),
    ci("erase", 0o177, t::VERASE),
    ci("kill", 0o25, t::VKILL),
    ci("eof", 0o04, t::VEOF),
    ci("eol", 0, t::VEOL),
    ci("eol2", 0, t::VEOL2),
    ci("swtch", 0, t::VSWTC),
    ci("start", 0o21, t::VSTART),
    ci("stop", 0o23, t::VSTOP),
    ci("susp", 0o32, t::VSUSP),
    ci("rprnt", 0o22, t::VREPRINT),
    ci("werase", 0o27, t::VWERASE),
    ci("lnext", 0o26, t::VLNEXT),
    ci("discard", 0o17, t::VDISCARD),
    ci("min", 1, t::VMIN),
    ci("time", 0, t::VTIME),
];

/// Quantos de `CONTROLS` são caracteres especiais (os que vêm antes de `min`).
const SPECIAL_CHARS: usize = 15;

/// Velocidades aceitas na linha de comando e o código `Bnnn` de cada uma.
const SPEEDS: &[(&str, u32)] = &[
    ("0", t::B0),
    ("50", t::B50),
    ("75", t::B75),
    ("110", t::B110),
    ("134", t::B134),
    ("134.5", t::B134),
    ("150", t::B150),
    ("200", t::B200),
    ("300", t::B300),
    ("600", t::B600),
    ("1200", t::B1200),
    ("1800", t::B1800),
    ("2400", t::B2400),
    ("4800", t::B4800),
    ("9600", t::B9600),
    ("19200", t::B19200),
    ("38400", t::B38400),
    ("exta", t::EXTA),
    ("extb", t::EXTB),
    ("57600", t::B57600),
    ("115200", t::B115200),
    ("230400", t::B230400),
    ("460800", t::B460800),
    ("500000", t::B500000),
    ("576000", t::B576000),
    ("921600", t::B921600),
    ("1000000", t::B1000000),
    ("1152000", t::B1152000),
    ("1500000", t::B1500000),
    ("2000000", t::B2000000),
    ("2500000", t::B2500000),
    ("3000000", t::B3000000),
    ("3500000", t::B3500000),
    ("4000000", t::B4000000),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedKind {
    Input,
    Output,
    Both,
}

fn string_to_baud(arg: &[u8]) -> Option<u32> {
    SPEEDS.iter().find(|(name, _)| name.as_bytes() == arg).map(|(_, code)| *code)
}

/// A taxa em bauds de um código `Bnnn`; zero pra o que não está na tabela (`BOTHER`).
fn baud_to_value(code: u32) -> u64 {
    SPEEDS
        .iter()
        .find(|(_, c)| *c == code)
        .and_then(|(name, _)| name.parse::<u64>().ok())
        .unwrap_or(0)
}

/// `cfgetispeed` da glibc: o campo `CIBAUD`, e sem ele a velocidade de saída.
fn ispeed_code(mode: &Termios) -> u32 {
    let input = (mode.c_cflag & t::CIBAUD) >> t::IBSHIFT;
    if input != 0 { input } else { mode.c_cflag & t::CBAUD }
}

fn set_speed(kind: SpeedKind, code: u32, mode: &mut Termios) {
    if kind != SpeedKind::Output {
        mode.c_cflag = (mode.c_cflag & !t::CIBAUD) | ((code & t::CBAUD) << t::IBSHIFT);
    }
    if kind != SpeedKind::Input {
        mode.c_cflag = (mode.c_cflag & !t::CBAUD) | (code & t::CBAUD);
    }
}

/// Deixa `c_ispeed`/`c_ospeed` de acordo com o que o `c_cflag` codifica (o contrato do `Termios`
/// pede isso pra taxas que não são `BOTHER`).
fn sync_speeds(mode: &mut Termios) {
    if let Some(v) = t::baud_of(mode.c_cflag & t::CBAUD) {
        mode.c_ospeed = v;
    }
    if let Some(v) = t::baud_of(ispeed_code(mode)) {
        mode.c_ispeed = v;
    }
}

fn flags_of(kind: Kind, mode: &Termios) -> Option<u32> {
    match kind {
        Kind::Control => Some(mode.c_cflag),
        Kind::Input => Some(mode.c_iflag),
        Kind::Output => Some(mode.c_oflag),
        Kind::Local => Some(mode.c_lflag),
        Kind::Combination => None,
    }
}

fn set_flags(kind: Kind, mode: &mut Termios, value: u32) {
    match kind {
        Kind::Control => mode.c_cflag = value,
        Kind::Input => mode.c_iflag = value,
        Kind::Output => mode.c_oflag = value,
        Kind::Local => mode.c_lflag = value,
        Kind::Combination => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Citação de nomes nas mensagens (o `quote` e o `quotef` do gnulib)

/// Um caractere de `s` a partir de `i`: o texto, quantos bytes ocupa e se é imprimível.
fn next_char(s: &[u8], i: usize) -> (Option<char>, usize, bool) {
    let b = s[i];
    if b < 0x80 {
        return (Some(b as char), 1, (0x20..0x7f).contains(&b));
    }
    for n in 2..=4 {
        if i + n <= s.len()
            && let Ok(text) = std::str::from_utf8(&s[i..i + n])
            && let Some(c) = text.chars().next()
        {
            return (Some(c), n, !c.is_control());
        }
    }
    (None, 1, false)
}

/// `shell_escape_always_quoting_style`: sempre entre aspas simples; o que não é imprimível sai em
/// `$'\ooo'`.
fn quote_always(s: &[u8]) -> String {
    #[derive(PartialEq)]
    enum Mode {
        Out,
        Single,
        Dollar,
    }
    let mut out = String::new();
    let mut mode = Mode::Out;
    let mut i = 0;
    while i < s.len() {
        let (ch, len, printable) = next_char(s, i);
        match (ch, printable) {
            (Some('\''), true) => {
                if mode != Mode::Out {
                    out.push('\'');
                }
                out.push_str("\\'");
                mode = Mode::Out;
            }
            (Some(c), true) => {
                match mode {
                    Mode::Out => out.push('\''),
                    Mode::Dollar => out.push_str("''"),
                    Mode::Single => {}
                }
                mode = Mode::Single;
                out.push(c);
            }
            _ => {
                match mode {
                    Mode::Out => out.push_str("$'"),
                    Mode::Single => out.push_str("'$'"),
                    Mode::Dollar => {}
                }
                mode = Mode::Dollar;
                for &byte in &s[i..i + len] {
                    match byte {
                        7 => out.push_str("\\a"),
                        8 => out.push_str("\\b"),
                        9 => out.push_str("\\t"),
                        10 => out.push_str("\\n"),
                        11 => out.push_str("\\v"),
                        12 => out.push_str("\\f"),
                        13 => out.push_str("\\r"),
                        other => out.push_str(&format!("\\{other:03o}")),
                    }
                }
            }
        }
        i += len;
    }
    match mode {
        Mode::Out => {
            if s.is_empty() {
                out.push_str("''");
            }
        }
        _ => out.push('\''),
    }
    out
}

/// O nome precisa de aspas pro `shell_escape_quoting_style`?
fn needs_quotes(s: &[u8]) -> bool {
    if s.is_empty() {
        return true;
    }
    for (i, &b) in s.iter().enumerate() {
        match b {
            b' ' | b'!' | b'"' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b';' | b'<' | b'='
            | b'>' | b'?' | b'[' | b'\\' | b'^' | b'`' | b'|' => return true,
            b'#' | b'~' if i == 0 => return true,
            b'{' | b'}' if s.len() == 1 => return true,
            0..=31 | 127 => return true,
            _ => {}
        }
    }
    match std::str::from_utf8(s) {
        Ok(text) => text.chars().any(char::is_control),
        Err(_) => true,
    }
}

/// `quotef`: entre aspas só quando o nome tem algo que o shell trataria.
fn quotef(s: &[u8]) -> String {
    if needs_quotes(s) { quote_always(s) } else { String::from_utf8_lossy(s).into_owned() }
}

// ---------------------------------------------------------------------------------------------
// Números

enum IntErr {
    Invalid,
    Overflow,
}

/// `xstrtoimax(s, NULL, 0, &v, "bB")` (com `allow_b`) ou sem sufixos: base 0, espaços e sinal no
/// começo, `b` multiplica por 512 e `B` por 1024.
fn parse_integer(s: &[u8], allow_b: bool) -> Result<i128, IntErr> {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let mut negative = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        negative = s[i] == b'-';
        i += 1;
    }
    let mut base: u32 = 10;
    if i + 2 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') && s[i + 2].is_ascii_hexdigit() {
        base = 16;
        i += 2;
    } else if i < s.len() && s[i] == b'0' {
        base = 8;
    }
    let start = i;
    let mut value: i128 = 0;
    while i < s.len() {
        let Some(d) = (s[i] as char).to_digit(base) else { break };
        value = value * i128::from(base) + i128::from(d);
        if value > (1i128 << 64) {
            value = 1i128 << 64;
        }
        i += 1;
    }
    if i == start {
        if allow_b && i < s.len() && (s[i] == b'b' || s[i] == b'B') {
            value = 1;
        } else {
            return Err(IntErr::Invalid);
        }
    }
    if i < s.len() {
        if !allow_b {
            return Err(IntErr::Invalid);
        }
        let factor: i128 = match s[i] {
            b'b' => 512,
            b'B' => 1024,
            _ => return Err(IntErr::Invalid),
        };
        i += 1;
        if i < s.len() {
            return Err(IntErr::Invalid);
        }
        value *= factor;
    }
    if negative {
        value = -value;
    }
    if value > i128::from(i64::MAX) || value < i128::from(i64::MIN) {
        return Err(IntErr::Overflow);
    }
    Ok(value)
}

/// `$COLUMNS` como o `screen_columns` do stty a lê: tudo ou nada, entre 1 e `INT_MAX`.
fn parse_columns(s: &[u8]) -> Option<usize> {
    match parse_integer(s, false) {
        Ok(n) if n > 0 && n <= i128::from(i32::MAX) => Some(n as usize),
        _ => None,
    }
}

/// A largura que a saída usa pra quebrar linha.
fn screen_columns() -> usize {
    if let Ok(win) = sysio::unistd::winsize(1)
        && win.cols > 0
    {
        return usize::from(win.cols);
    }
    if let Some(value) = sysio::env::var_os("COLUMNS")
        && let Some(n) = parse_columns(value.as_bytes())
    {
        return n;
    }
    80
}

// ---------------------------------------------------------------------------------------------
// Estado do programa e mensagens

struct App {
    prog: String,
    out: Vec<u8>,
    col: usize,
    max_col: usize,
}

impl App {
    fn new(prog: String) -> App {
        App { prog, out: Vec::new(), col: 0, max_col: 80 }
    }

    /// `error (EXIT_FAILURE, 0, ...)`: a mensagem e o código 1.
    fn die(&self, message: &str) -> i32 {
        sysio::eprintln!("{}: {}", self.prog, message);
        1
    }

    /// `error (EXIT_FAILURE, errno, "%s", quotef (name))`.
    fn die_errno(&self, name: &[u8], errno: Errno) -> i32 {
        sysio::eprintln!("{}: {}: {}", self.prog, quotef(name), errno.message());
        1
    }

    /// `error (0, 0, ...)` seguido de `usage (EXIT_FAILURE)`.
    fn usage_failure(&self, message: &str) -> i32 {
        sysio::eprintln!("{}: {}", self.prog, message);
        sysio::eprintln!("Try '{} --help' for more information.", self.prog);
        1
    }

    /// `wrapf`: escreve a mensagem separada da anterior por um espaço, ou quebra a linha se não
    /// couber na largura.
    fn wrapf(&mut self, message: &str) {
        let len = message.len();
        if self.col > 0 {
            if self.max_col.saturating_sub(self.col) < len {
                self.out.push(b'\n');
                self.col = 0;
            } else {
                self.out.push(b' ');
                self.col += 1;
            }
        }
        self.out.extend_from_slice(message.as_bytes());
        self.col += len;
    }

    fn newline(&mut self) {
        self.out.push(b'\n');
        self.col = 0;
    }
}

/// O terminal em uso: o `sysabi` do processo, o fd e o nome pras mensagens.
struct Dev {
    sys: Arc<dyn Syscalls>,
    fd: Fd,
    name: Vec<u8>,
}

fn write_stdout(bytes: &[u8]) {
    use sysio::io::Write as _;
    let _ = sysio::io::stdout().write_all(bytes);
}

// ---------------------------------------------------------------------------------------------
// Exibição

/// `visible`: o caractere de controle em notação de circunflexo.
fn visible(ch: u8) -> String {
    if ch == 0 {
        return "<undef>".to_string();
    }
    let mut out = String::new();
    let mut c = ch;
    if c >= 128 {
        out.push_str("M-");
        c -= 128;
    }
    if c < 32 {
        out.push('^');
        out.push(char::from(c + 64));
    } else if c < 127 {
        out.push(char::from(c));
    } else {
        out.push_str("^?");
    }
    out
}

fn display_speed(app: &mut App, mode: &Termios, fancy: bool) {
    let ospeed = mode.c_cflag & t::CBAUD;
    let ispeed = ispeed_code(mode);
    let text = if ispeed == 0 || ispeed == ospeed {
        if fancy {
            format!("speed {} baud;", baud_to_value(ospeed))
        } else {
            format!("{}\n", baud_to_value(ospeed))
        }
    } else if fancy {
        format!("ispeed {} baud; ospeed {} baud;", baud_to_value(ispeed), baud_to_value(ospeed))
    } else {
        format!("{} {}\n", baud_to_value(ispeed), baud_to_value(ospeed))
    };
    app.wrapf(&text);
    if !fancy {
        app.col = 0;
    }
}

/// Lê o tamanho da janela; `EINVAL` (dispositivo sem tamanho) é `Ok(None)`.
fn get_win_size(dev: &Dev) -> Result<Option<Winsize>, Errno> {
    match dev.sys.tcgetwinsize(dev.fd) {
        Ok(win) => Ok(Some(win)),
        Err(e) if e == Errno::EINVAL => Ok(None),
        Err(e) => Err(e),
    }
}

/// `display_window_size`: `rows N; columns N;` no `-a`, `N N` no `size`.
fn display_window_size(app: &mut App, dev: &Dev, fancy: bool) -> Result<(), i32> {
    match get_win_size(dev) {
        Err(e) => Err(app.die_errno(&dev.name, e)),
        Ok(None) => {
            if fancy {
                Ok(())
            } else {
                Err(app.die(&format!("{}: no size information for this device", quotef(&dev.name))))
            }
        }
        Ok(Some(win)) => {
            let text = if fancy {
                format!("rows {}; columns {};", win.rows, win.cols)
            } else {
                format!("{} {}\n", win.rows, win.cols)
            };
            app.wrapf(&text);
            if !fancy {
                app.col = 0;
            }
            Ok(())
        }
    }
}

fn display_recoverable(app: &mut App, mode: &Termios) {
    let mut text = format!("{:x}:{:x}:{:x}:{:x}", mode.c_iflag, mode.c_oflag, mode.c_cflag, mode.c_lflag);
    for i in 0..GLIBC_NCCS {
        let value = if i < t::NCCS { mode.c_cc[i] } else { 0 };
        text.push_str(&format!(":{value:x}"));
    }
    text.push('\n');
    app.out.extend_from_slice(text.as_bytes());
}

/// A saída padrão: velocidade, disciplina de linha e o que difere do `sane`.
fn display_changed(app: &mut App, mode: &Termios) {
    display_speed(app, mode, true);
    app.wrapf(&format!("line = {};", mode.c_line));
    app.newline();

    let mut empty_line = true;
    for info in &CONTROLS[..SPECIAL_CHARS] {
        if mode.c_cc[info.offset] == info.sane {
            continue;
        }
        empty_line = false;
        app.wrapf(&format!("{} = {};", info.name, visible(mode.c_cc[info.offset])));
    }
    if mode.c_lflag & t::ICANON == 0 {
        app.wrapf(&format!("min = {}; time = {};", mode.c_cc[t::VMIN], mode.c_cc[t::VTIME]));
        empty_line = false;
    }
    if !empty_line {
        app.newline();
    }

    let mut empty_line = true;
    let mut prev = Kind::Control;
    for info in MODES {
        if info.flags & OMIT != 0 {
            continue;
        }
        if info.kind != prev {
            if !empty_line {
                app.newline();
                empty_line = true;
            }
            prev = info.kind;
        }
        let Some(word) = flags_of(info.kind, mode) else { continue };
        let mask = if info.mask != 0 { info.mask } else { info.bits };
        if word & mask == info.bits {
            if info.flags & SANE_UNSET != 0 {
                app.wrapf(info.name);
                empty_line = false;
            }
        } else if info.flags & (SANE_SET | REV) == (SANE_SET | REV) {
            app.wrapf(&format!("-{}", info.name));
            empty_line = false;
        }
    }
    if !empty_line {
        app.newline();
    }
}

/// `-a`: tudo, em linhas por tipo de configuração.
fn display_all(app: &mut App, dev: &Dev, mode: &Termios) -> Result<(), i32> {
    display_speed(app, mode, true);
    display_window_size(app, dev, true)?;
    app.wrapf(&format!("line = {};", mode.c_line));
    app.newline();

    for info in &CONTROLS[..SPECIAL_CHARS] {
        app.wrapf(&format!("{} = {};", info.name, visible(mode.c_cc[info.offset])));
    }
    app.wrapf(&format!("min = {}; time = {};", mode.c_cc[t::VMIN], mode.c_cc[t::VTIME]));
    if app.col != 0 {
        app.newline();
    }

    let mut prev = Kind::Control;
    for info in MODES {
        if info.flags & OMIT != 0 {
            continue;
        }
        if info.kind != prev {
            app.newline();
            prev = info.kind;
        }
        let Some(word) = flags_of(info.kind, mode) else { continue };
        let mask = if info.mask != 0 { info.mask } else { info.bits };
        if word & mask == info.bits {
            app.wrapf(info.name);
        } else if info.flags & REV != 0 {
            app.wrapf(&format!("-{}", info.name));
        }
    }
    app.newline();
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Configurações

fn sane_mode(mode: &mut Termios) {
    for info in CONTROLS {
        mode.c_cc[info.offset] = info.sane;
    }
    for info in MODES {
        if info.flags & (SANE_SET | SANE_UNSET) == 0 {
            continue;
        }
        let Some(word) = flags_of(info.kind, mode) else { continue };
        let new = if info.flags & SANE_SET != 0 {
            (word & !info.mask) | info.bits
        } else {
            word & !info.mask & !info.bits
        };
        set_flags(info.kind, mode, new);
    }
}

/// `set_mode`: aplica uma configuração (ou a forma negada). Devolve falso quando a configuração não
/// tem forma negada.
fn set_mode(info: &ModeInfo, reversed: bool, mode: &mut Termios, when: &mut SetAttrWhen) -> bool {
    if reversed && info.flags & REV == 0 {
        return false;
    }
    if let Some(word) = flags_of(info.kind, mode) {
        let new = if reversed {
            word & !info.mask & !info.bits
        } else {
            (word & !info.mask) | info.bits
        };
        set_flags(info.kind, mode, new);
        return true;
    }
    match info.name {
        "drain" => *when = if reversed { SetAttrWhen::Now } else { SetAttrWhen::Drain },
        "evenp" | "parity" => {
            if reversed {
                mode.c_cflag = (mode.c_cflag & !t::PARENB & !t::CSIZE) | t::CS8;
            } else {
                mode.c_cflag = (mode.c_cflag & !t::PARODD & !t::CSIZE) | t::CS7 | t::PARENB;
            }
        }
        "oddp" => {
            if reversed {
                mode.c_cflag = (mode.c_cflag & !t::PARENB & !t::CSIZE) | t::CS8;
            } else {
                mode.c_cflag = (mode.c_cflag & !t::CSIZE) | t::CS7 | t::PARODD | t::PARENB;
            }
        }
        "nl" => {
            if reversed {
                mode.c_iflag = (mode.c_iflag | t::ICRNL) & !t::INLCR & !t::IGNCR;
                mode.c_oflag = (mode.c_oflag | t::ONLCR) & !t::OCRNL & !t::ONLRET;
            } else {
                mode.c_iflag &= !t::ICRNL;
                mode.c_oflag &= !t::ONLCR;
            }
        }
        "ek" => {
            mode.c_cc[t::VERASE] = 0o177;
            mode.c_cc[t::VKILL] = 0o25;
        }
        "sane" => sane_mode(mode),
        "cbreak" => {
            if reversed {
                mode.c_lflag |= t::ICANON;
            } else {
                mode.c_lflag &= !t::ICANON;
            }
        }
        "pass8" => {
            if reversed {
                mode.c_cflag = (mode.c_cflag & !t::CSIZE) | t::CS7 | t::PARENB;
                mode.c_iflag |= t::ISTRIP;
            } else {
                mode.c_cflag = (mode.c_cflag & !t::PARENB & !t::CSIZE) | t::CS8;
                mode.c_iflag &= !t::ISTRIP;
            }
        }
        "litout" => {
            if reversed {
                mode.c_cflag = (mode.c_cflag & !t::CSIZE) | t::CS7 | t::PARENB;
                mode.c_iflag |= t::ISTRIP;
                mode.c_oflag |= t::OPOST;
            } else {
                mode.c_cflag = (mode.c_cflag & !t::PARENB & !t::CSIZE) | t::CS8;
                mode.c_iflag &= !t::ISTRIP;
                mode.c_oflag &= !t::OPOST;
            }
        }
        "raw" | "cooked" => {
            let cooked = (info.name == "raw" && reversed) || (info.name == "cooked" && !reversed);
            if cooked {
                mode.c_iflag |= t::BRKINT | t::IGNPAR | t::ISTRIP | t::ICRNL | t::IXON;
                mode.c_oflag |= t::OPOST;
                mode.c_lflag |= t::ISIG | t::ICANON;
            } else {
                mode.c_iflag = 0;
                mode.c_oflag &= !t::OPOST;
                mode.c_lflag &= !(t::ISIG | t::ICANON | t::XCASE);
                mode.c_cc[t::VMIN] = 1;
                mode.c_cc[t::VTIME] = 0;
            }
        }
        "decctlq" => {
            if reversed {
                mode.c_iflag |= t::IXANY;
            } else {
                mode.c_iflag &= !t::IXANY;
            }
        }
        "tabs" => {
            if reversed {
                mode.c_oflag = (mode.c_oflag & !t::TABDLY) | t::TAB3;
            } else {
                mode.c_oflag = (mode.c_oflag & !t::TABDLY) | t::TAB0;
            }
        }
        "lcase" | "LCASE" => {
            if reversed {
                mode.c_lflag &= !t::XCASE;
                mode.c_iflag &= !t::IUCLC;
                mode.c_oflag &= !t::OLCUC;
            } else {
                mode.c_lflag |= t::XCASE;
                mode.c_iflag |= t::IUCLC;
                mode.c_oflag |= t::OLCUC;
            }
        }
        "crt" => mode.c_lflag |= t::ECHOE | t::ECHOCTL | t::ECHOKE,
        "dec" => {
            mode.c_cc[t::VINTR] = 3;
            mode.c_cc[t::VERASE] = 127;
            mode.c_cc[t::VKILL] = 21;
            mode.c_lflag |= t::ECHOE | t::ECHOCTL | t::ECHOKE;
            mode.c_iflag &= !t::IXANY;
        }
        _ => {}
    }
    true
}

/// `integer_arg`: inteiro entre 0 e `max`, com o sufixo `b`/`B`; erro morre com a mensagem do stty.
fn integer_arg(app: &App, arg: &[u8], max: u64) -> Result<u64, i32> {
    match parse_integer(arg, true) {
        Ok(v) if v >= 0 && v <= i128::from(max) => Ok(v as u64),
        Ok(_) | Err(IntErr::Overflow) => Err(app.die(&format!(
            "{}: {}",
            quote_always(arg),
            Errno::EOVERFLOW.message()
        ))),
        Err(IntErr::Invalid) => Err(app.die(&format!("invalid integer argument {}", quote_always(arg)))),
    }
}

/// `set_control_char`: `^c`, `^?`, `undef`, `^-`, um caractere literal ou um número.
fn set_control_char(app: &App, info: &ControlInfo, arg: &[u8], mode: &mut Termios) -> Result<(), i32> {
    let value: u8 = if info.name == "min" || info.name == "time" {
        integer_arg(app, arg, 255)? as u8
    } else if arg.len() <= 1 {
        arg.first().copied().unwrap_or(0)
    } else if arg == b"^-" || arg == b"undef" {
        0
    } else if arg[0] == b'^' {
        if arg[1] == b'?' { 127 } else { arg[1] & !0o140 }
    } else {
        integer_arg(app, arg, 255)? as u8
    };
    mode.c_cc[info.offset] = value;
    Ok(())
}

/// `strtoul(s, &p, 16)`: o valor, quantos bytes consumiu e se estourou.
fn strtoul16(s: &[u8]) -> (u64, usize, bool) {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let mut negative = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        negative = s[i] == b'-';
        i += 1;
    }
    if i + 2 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') && s[i + 2].is_ascii_hexdigit() {
        i += 2;
    }
    let start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let Some(d) = (s[i] as char).to_digit(16) else { break };
        match value.checked_mul(16).and_then(|v| v.checked_add(u64::from(d))) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == start {
        return (0, 0, false);
    }
    if negative {
        value = value.wrapping_neg();
    }
    (value, i, overflow)
}

/// `recover_mode`: o formato do `-g` (`iflag:oflag:cflag:lflag:` e os 32 caracteres, em hexa).
fn recover_mode(arg: &[u8], mode: &mut Termios) -> bool {
    let mut flags = [0u32; 4];
    let mut pos = 0;
    for slot in &mut flags {
        let (value, used, overflow) = strtoul16(&arg[pos..]);
        let end = pos + used;
        if overflow || value > u64::from(u32::MAX) || arg.get(end) != Some(&b':') {
            return false;
        }
        *slot = value as u32;
        pos = end + 1;
    }
    let mut cc = [0u8; GLIBC_NCCS];
    for (i, slot) in cc.iter_mut().enumerate() {
        let (value, used, _) = strtoul16(&arg[pos..]);
        let end = pos + used;
        let last = i == GLIBC_NCCS - 1;
        let sep = arg.get(end).copied();
        if value > 255 || (last && sep.is_some()) || (!last && sep != Some(b':')) {
            return false;
        }
        *slot = value as u8;
        pos = end + 1;
    }
    mode.c_iflag = flags[0];
    mode.c_oflag = flags[1];
    mode.c_cflag = flags[2];
    mode.c_lflag = flags[3];
    mode.c_cc.copy_from_slice(&cc[..t::NCCS]);
    true
}

/// `set_window_size`: `rows` e `cols` negativos deixam o valor como está.
fn set_window_size(app: &App, dev: &Dev, rows: i64, cols: i64) -> Result<(), i32> {
    let mut win = match get_win_size(dev) {
        Ok(Some(win)) => win,
        Ok(None) => Winsize::default(),
        Err(e) => return Err(app.die_errno(&dev.name, e)),
    };
    if rows >= 0 {
        win.rows = rows as u16;
    }
    if cols >= 0 {
        win.cols = cols as u16;
    }
    dev.sys.tcsetwinsize(dev.fd, win).map_err(|e| app.die_errno(&dev.name, e))
}

/// O que o `tcsetattr` aplicou é o que se pediu? (`CIBAUD` e as velocidades numéricas ficam de fora:
/// o kernel as recalcula.)
fn same_mode(a: &Termios, b: &Termios) -> bool {
    a.c_iflag == b.c_iflag
        && a.c_oflag == b.c_oflag
        && (a.c_cflag & !t::CIBAUD) == (b.c_cflag & !t::CIBAUD)
        && a.c_lflag == b.c_lflag
        && a.c_line == b.c_line
        && a.c_cc == b.c_cc
}

// ---------------------------------------------------------------------------------------------
// Programa

/// As opções longas da primeira passada.
fn find_long(name: &[u8]) -> Option<&'static str> {
    static LONGS: [&str; 5] = ["all", "save", "file", "help", "version"];
    if let Some(exact) = LONGS.iter().copied().find(|l| l.as_bytes() == name) {
        return Some(exact);
    }
    let mut found = LONGS.iter().copied().filter(|l| !name.is_empty() && l.as_bytes().starts_with(name));
    match (found.next(), found.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let args = args.to_vec();
    sysio::run(move || run(&args))
}

fn run(args: &[OsString]) -> i32 {
    let mut prog = sysio::proc::program_name();
    if prog.is_empty() {
        prog = "stty".to_string();
    }
    let mut app = App::new(prog);
    let code = match real_main(&mut app, args) {
        Ok(()) => 0,
        Err(code) => code,
    };
    write_stdout(&app.out);
    code
}

fn real_main(app: &mut App, args: &[OsString]) -> Result<(), i32> {
    let argv: Vec<&[u8]> = args.iter().map(|a| a.as_bytes()).collect();
    let argc = argv.len();
    let mut consumed = vec![false; argc];
    let mut verbose = false;
    let mut recoverable = false;
    let mut noargs = true;
    let mut device: Option<Vec<u8>> = None;

    // Primeira passada: só -a, -g, -F e as opções longas; o resto fica pra segunda.
    let mut i = 1;
    while i < argc {
        let idx = i;
        let arg = argv[idx];
        i += 1;
        if arg == b"--" {
            consumed[idx] = true;
            if i < argc {
                noargs = false;
            }
            break;
        }
        if let Some(body) = arg.strip_prefix(b"--") {
            let (name, value) = match body.iter().position(|&b| b == b'=') {
                Some(p) => (&body[..p], Some(&body[p + 1..])),
                None => (body, None),
            };
            match find_long(name) {
                Some("all") if value.is_none() => {
                    verbose = true;
                    consumed[idx] = true;
                }
                Some("save") if value.is_none() => {
                    recoverable = true;
                    consumed[idx] = true;
                }
                Some("file") => {
                    let given = match value {
                        Some(v) => Some(v.to_vec()),
                        None if i < argc => {
                            consumed[i] = true;
                            i += 1;
                            Some(argv[i - 1].to_vec())
                        }
                        None => None,
                    };
                    match given {
                        Some(v) => {
                            if device.is_some() {
                                return Err(app.die("only one device may be specified"));
                            }
                            device = Some(v);
                            consumed[idx] = true;
                        }
                        None => noargs = false,
                    }
                }
                Some("help") if value.is_none() => {
                    app.out.extend_from_slice(HELP.as_bytes());
                    return Err(0);
                }
                Some("version") if value.is_none() => {
                    app.out.extend_from_slice(VERSION.as_bytes());
                    return Err(0);
                }
                _ => noargs = false,
            }
            continue;
        }
        if arg.len() > 1 && arg[0] == b'-' {
            let cluster = &arg[1..];
            let mut ok = true;
            let mut k = 0;
            while k < cluster.len() {
                match cluster[k] {
                    b'a' => verbose = true,
                    b'g' => recoverable = true,
                    b'F' => {
                        let rest = &cluster[k + 1..];
                        let given = if !rest.is_empty() {
                            Some(rest.to_vec())
                        } else if i < argc {
                            consumed[i] = true;
                            i += 1;
                            Some(argv[i - 1].to_vec())
                        } else {
                            None
                        };
                        match given {
                            Some(v) => {
                                if device.is_some() {
                                    return Err(app.die("only one device may be specified"));
                                }
                                device = Some(v);
                            }
                            None => ok = false,
                        }
                        break;
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
                k += 1;
            }
            if ok {
                consumed[idx] = true;
            } else {
                noargs = false;
            }
            continue;
        }
        noargs = false;
    }

    if verbose && recoverable {
        return Err(app.die("the options for verbose and stty-readable output styles are mutually exclusive"));
    }
    if !noargs && (verbose || recoverable) {
        return Err(app.die("when specifying an output style, modes may not be set"));
    }

    let sys = sysio::proc::sys();
    let dev = match device {
        Some(path) => {
            let fd = match sys.openat(Fd::CWD, &path, OFlags::RDONLY | OFlags::NONBLOCK, 0) {
                Ok(fd) => fd,
                Err(e) => return Err(app.die_errno(&path, e)),
            };
            if sys.set_status_flags(fd, OFlags::empty()).is_err() {
                return Err(app.die(&format!("{}: couldn't reset non-blocking mode", quotef(&path))));
            }
            Dev { sys, fd, name: path }
        }
        None => Dev { sys, fd: Fd::STDIN, name: b"standard input".to_vec() },
    };

    let mut mode = match dev.sys.tcgetattr(dev.fd) {
        Ok(mode) => mode,
        Err(e) => return Err(app.die_errno(&dev.name, e)),
    };

    if verbose || recoverable || noargs {
        app.max_col = screen_columns();
        app.col = 0;
        if verbose {
            display_all(app, &dev, &mode)?;
        } else if recoverable {
            display_recoverable(app, &mode);
        } else {
            display_changed(app, &mode);
        }
        return Ok(());
    }

    // Segunda passada: as configurações, na ordem.
    let mut require_set_attr = false;
    let mut when = SetAttrWhen::Drain;
    let mut k = 1;
    while k < argc {
        if consumed[k] {
            k += 1;
            continue;
        }
        let original = argv[k];
        let mut arg = original;
        let mut reversed = false;
        if arg.first() == Some(&b'-') {
            arg = &arg[1..];
            reversed = true;
        }
        let mut matched = false;
        for info in MODES {
            if info.name.as_bytes() == arg {
                matched = set_mode(info, reversed, &mut mode, &mut when);
                require_set_attr = true;
                break;
            }
        }
        if !matched && reversed {
            return Err(app.usage_failure(&format!("invalid argument {}", quote_always(original))));
        }
        if !matched {
            for info in CONTROLS {
                if info.name.as_bytes() == arg {
                    if k == argc - 1 {
                        return Err(app.usage_failure(&format!("missing argument to {}", quote_always(arg))));
                    }
                    matched = true;
                    k += 1;
                    set_control_char(app, info, argv[k], &mut mode)?;
                    require_set_attr = true;
                    break;
                }
            }
        }
        if !matched {
            if arg == b"ispeed" || arg == b"ospeed" {
                if k == argc - 1 {
                    return Err(app.usage_failure(&format!("missing argument to {}", quote_always(arg))));
                }
                k += 1;
                let Some(code) = string_to_baud(argv[k]) else {
                    return Err(app.die(&format!("invalid argument {}", quote_always(argv[k]))));
                };
                let kind = if arg == b"ispeed" { SpeedKind::Input } else { SpeedKind::Output };
                set_speed(kind, code, &mut mode);
                require_set_attr = true;
            } else if arg == b"rows" || arg == b"cols" || arg == b"columns" {
                if k == argc - 1 {
                    return Err(app.usage_failure(&format!("missing argument to {}", quote_always(arg))));
                }
                k += 1;
                let value = integer_arg(app, argv[k], i32::MAX as u64)? as i64;
                if arg == b"rows" {
                    set_window_size(app, &dev, value, -1)?;
                } else {
                    set_window_size(app, &dev, -1, value)?;
                }
            } else if arg == b"size" {
                app.max_col = screen_columns();
                app.col = 0;
                display_window_size(app, &dev, false)?;
            } else if arg == b"line" {
                if k == argc - 1 {
                    return Err(app.usage_failure(&format!("missing argument to {}", quote_always(arg))));
                }
                k += 1;
                let value = integer_arg(app, argv[k], i64::MAX as u64)?;
                mode.c_line = value as u8;
                require_set_attr = true;
            } else if arg == b"speed" {
                app.max_col = screen_columns();
                display_speed(app, &mode, false);
            } else if let Some(code) = string_to_baud(arg) {
                set_speed(SpeedKind::Both, code, &mut mode);
                require_set_attr = true;
            } else if recover_mode(arg, &mut mode) {
                require_set_attr = true;
            } else {
                return Err(app.usage_failure(&format!("invalid argument {}", quote_always(arg))));
            }
        }
        k += 1;
    }

    if require_set_attr {
        sync_speeds(&mut mode);
        if let Err(e) = dev.sys.tcsetattr(dev.fd, when, &mode) {
            return Err(app.die_errno(&dev.name, e));
        }
        // O `tcsetattr` pode aplicar só parte do pedido e dizer que deu certo: confere o que ficou.
        let new_mode = match dev.sys.tcgetattr(dev.fd) {
            Ok(new_mode) => new_mode,
            Err(e) => return Err(app.die_errno(&dev.name, e)),
        };
        if !same_mode(&mode, &new_mode) {
            return Err(app.die(&format!("{}: unable to perform all requested operations", quotef(&dev.name))));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(app: &App) -> String {
        String::from_utf8_lossy(&app.out).into_owned()
    }

    #[test]
    fn visible_chars() {
        assert_eq!(visible(0), "<undef>");
        assert_eq!(visible(3), "^C");
        assert_eq!(visible(0o177), "^?");
        assert_eq!(visible(b'a'), "a");
        assert_eq!(visible(0x80), "M-^@");
        assert_eq!(visible(0xff), "M-^?");
    }

    #[test]
    fn default_display_changed() {
        let mut app = App::new("stty".to_string());
        display_changed(&mut app, &Termios::default());
        assert_eq!(rendered(&app), "speed 38400 baud; line = 0;\n-brkint -imaxbel\n");
    }

    #[test]
    fn raw_shows_min_time() {
        let mut mode = Termios::default();
        let mut when = SetAttrWhen::Drain;
        let raw = MODES.iter().find(|m| m.name == "raw").unwrap();
        assert!(set_mode(raw, false, &mut mode, &mut when));
        let mut app = App::new("stty".to_string());
        display_changed(&mut app, &mode);
        assert!(rendered(&app).starts_with("speed 38400 baud; line = 0;\nmin = 1; time = 0;\n"));
    }

    #[test]
    fn save_roundtrip() {
        let mode = Termios::default();
        let mut app = App::new("stty".to_string());
        display_recoverable(&mut app, &mode);
        let text = rendered(&app);
        let expected = format!("500:5:4bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16{}\n", ":0".repeat(16));
        assert_eq!(text, expected);
        let mut other = Termios::default();
        other.c_lflag = 0;
        assert!(recover_mode(text.trim_end().as_bytes(), &mut other));
        assert_eq!(other.c_lflag, mode.c_lflag);
        assert_eq!(other.c_cc, mode.c_cc);
    }

    #[test]
    fn recover_rejects_garbage() {
        let mut mode = Termios::default();
        assert!(!recover_mode(b"nonsense", &mut mode));
        assert!(!recover_mode(b"500:5:4bf:8a3b:3", &mut mode));
        let too_many = format!("500:5:4bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16{}", ":0".repeat(17));
        assert!(!recover_mode(too_many.as_bytes(), &mut mode));
    }

    #[test]
    fn sane_restores_defaults() {
        let mut mode = Termios::default();
        mode.c_lflag = 0;
        mode.c_iflag = t::ISTRIP | t::IXOFF;
        mode.c_cc[t::VINTR] = 0;
        let mut when = SetAttrWhen::Drain;
        let sane = MODES.iter().find(|m| m.name == "sane").unwrap();
        assert!(set_mode(sane, false, &mut mode, &mut when));
        assert_eq!(mode.c_cc[t::VINTR], 3);
        assert_ne!(mode.c_lflag & t::ICANON, 0);
        assert_eq!(mode.c_iflag & t::IXOFF, 0);
        assert_ne!(mode.c_iflag & t::ISTRIP, 0);
        assert!(!set_mode(sane, true, &mut mode, &mut when));
    }

    #[test]
    fn control_chars() {
        let app = App::new("stty".to_string());
        let mut mode = Termios::default();
        let intr = &CONTROLS[0];
        set_control_char(&app, intr, b"^Z", &mut mode).unwrap();
        assert_eq!(mode.c_cc[t::VINTR], 0x1a);
        set_control_char(&app, intr, b"^?", &mut mode).unwrap();
        assert_eq!(mode.c_cc[t::VINTR], 127);
        set_control_char(&app, intr, b"undef", &mut mode).unwrap();
        assert_eq!(mode.c_cc[t::VINTR], 0);
        set_control_char(&app, intr, b"0x7e", &mut mode).unwrap();
        assert_eq!(mode.c_cc[t::VINTR], 0x7e);
        set_control_char(&app, intr, b"x", &mut mode).unwrap();
        assert_eq!(mode.c_cc[t::VINTR], b'x');
    }

    #[test]
    fn integers() {
        assert!(matches!(parse_integer(b"10", true), Ok(10)));
        assert!(matches!(parse_integer(b"0x10", true), Ok(16)));
        assert!(matches!(parse_integer(b"010", true), Ok(8)));
        assert!(matches!(parse_integer(b"2b", true), Ok(1024)));
        assert!(matches!(parse_integer(b"x", true), Err(IntErr::Invalid)));
        assert!(matches!(parse_integer(b"", true), Err(IntErr::Invalid)));
        assert!(matches!(parse_integer(b"99999999999999999999999", true), Err(IntErr::Overflow)));
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_always(b"x"), "'x'");
        assert_eq!(quotef(b"standard input"), "'standard input'");
        assert_eq!(quotef(b"/dev/pts/3"), "/dev/pts/3");
        assert_eq!(quote_always(b"a'b"), "'a'\\''b'");
    }

    #[test]
    fn wrap_breaks_lines() {
        let mut app = App::new("stty".to_string());
        app.max_col = 20;
        app.wrapf("aaaaaaaaaa");
        app.wrapf("bbbbbbbb");
        app.wrapf("cc");
        assert_eq!(rendered(&app), "aaaaaaaaaa bbbbbbbb\ncc");
    }
}
