//! `/proc/<pid>/net` (e `/proc/net`, que aponta para `self/net`): o namespace de rede de um container
//! sem rede, só com o `lo`. O conteúdo foi capturado do oráculo (`docker run --network none`); o
//! `softnet_stat` tem uma linha por CPU e é cortado no número de CPUs da máquina.

use super::data::{ProcProvider, TcpSock, UdpSockRow, UnixSockRow};

/// Uma entrada da árvore. `parent` 0 é o próprio `net`; os outros são o índice (1 em diante) do
/// diretório que a contém.
pub(super) struct NetEnt {
    pub name: &'static str,
    pub parent: u32,
    pub dir: bool,
    pub mode: u32,
    pub data: &'static [u8],
}

macro_rules! f {
    ($name:literal, $mode:literal) => {
        NetEnt { name: $name, parent: 0, dir: false, mode: $mode, data: include_bytes!(concat!("net/", $name)) }
    };
    ($name:literal, $parent:expr, $path:literal, $mode:literal) => {
        NetEnt { name: $name, parent: $parent, dir: false, mode: $mode, data: include_bytes!(concat!("net/", $path)) }
    };
}

macro_rules! d {
    ($name:literal) => {
        NetEnt { name: $name, parent: 0, dir: true, mode: 0o555, data: b"" }
    };
}

/// Índices fixos dos subdiretórios (posição na tabela, a partir de 1).
const DEV_SNMP6: u32 = 1;
const NETFILTER: u32 = 2;
const STAT: u32 = 3;
const VLAN: u32 = 4;

pub(super) const SOFTNET_STAT: &str = "softnet_stat";

pub(super) static NET: &[NetEnt] = &[
    d!("dev_snmp6"),
    d!("netfilter"),
    d!("stat"),
    d!("vlan"),
    f!("lo", DEV_SNMP6, "dev_snmp6/lo", 0o444),
    f!("nf_log", NETFILTER, "netfilter/nf_log", 0o444),
    f!("rt_cache", STAT, "stat/rt_cache", 0o444),
    f!("nf_conntrack", STAT, "stat/nf_conntrack", 0o444),
    f!("config", VLAN, "vlan/config", 0o600),
    f!("anycast6", 0o444),
    f!("arp", 0o444),
    f!("dev", 0o444),
    f!("dev_mcast", 0o444),
    f!("fib_trie", 0o444),
    f!("fib_triestat", 0o444),
    f!("icmp", 0o444),
    f!("icmp6", 0o444),
    f!("if_inet6", 0o444),
    f!("igmp", 0o444),
    f!("igmp6", 0o444),
    f!("ip6_flowlabel", 0o444),
    f!("ip6_mr_cache", 0o444),
    f!("ip6_mr_vif", 0o444),
    f!("ip_mr_cache", 0o444),
    f!("ip_mr_vif", 0o444),
    f!("ip_tables_matches", 0o440),
    f!("ip_tables_names", 0o440),
    f!("ip_tables_targets", 0o440),
    f!("ipv6_route", 0o444),
    f!("mcfilter", 0o444),
    f!("mcfilter6", 0o444),
    f!("netlink", 0o444),
    f!("netstat", 0o444),
    f!("nf_conntrack", 0o440),
    f!("nf_conntrack_expect", 0o440),
    f!("packet", 0o444),
    f!("protocols", 0o444),
    f!("psched", 0o444),
    f!("ptype", 0o444),
    f!("raw", 0o444),
    f!("raw6", 0o444),
    f!("route", 0o444),
    f!("rt6_stats", 0o444),
    f!("rt_acct", 0o444),
    f!("rt_cache", 0o444),
    f!("snmp", 0o444),
    f!("snmp6", 0o444),
    f!("sockstat", 0o444),
    f!("sockstat6", 0o444),
    f!("softnet_stat", 0o444),
    f!("tcp", 0o444),
    f!("tcp6", 0o444),
    f!("udp", 0o444),
    f!("udp6", 0o444),
    f!("udplite", 0o444),
    f!("udplite6", 0o444),
    f!("unix", 0o444),
    f!("wireless", 0o444),
    f!("xfrm_stat", 0o444),
];

/// A entrada de índice `i` (1 em diante).
pub(super) fn ent(i: u32) -> Option<&'static NetEnt> {
    NET.get(i.checked_sub(1)? as usize)
}

/// O filho `name` do diretório `parent` (0 é o `net`).
pub(super) fn child(parent: u32, name: &[u8]) -> Option<u32> {
    NET.iter().position(|e| e.parent == parent && e.name.as_bytes() == name).map(|p| p as u32 + 1)
}

/// Os filhos de `parent`, na ordem da árvore de `proc_dir_entry` (tamanho do nome, depois bytes).
pub(super) fn children(parent: u32) -> Vec<u32> {
    let mut v: Vec<u32> = (1..=NET.len() as u32).filter(|&i| NET[i as usize - 1].parent == parent).collect();
    v.sort_by(|&a, &b| {
        let (x, y) = (NET[a as usize - 1].name, NET[b as usize - 1].name);
        (x.len(), x).cmp(&(y.len(), y))
    });
    v
}

/// Conteúdo de um arquivo. `tcp`, `tcp6`, `unix` e o `sockstat` saem das tabelas de sockets do kernel.
pub(super) fn content(i: u32, p: &dyn ProcProvider) -> Option<Vec<u8>> {
    let ncpus = p.ncpus();
    let socks = || p.tcp_socks();
    let e = ent(i).filter(|e| !e.dir)?;
    if e.parent == 0 && e.name == SOFTNET_STAT {
        let mut out = Vec::new();
        for l in e.data.split_inclusive(|&b| b == b'\n').take(ncpus.max(1) as usize) {
            out.extend_from_slice(l);
        }
        return Some(out);
    }
    if e.parent == 0 {
        match e.name {
            "tcp" => return Some(tcp_table(&socks(), false)),
            "tcp6" => return Some(tcp_table(&socks(), true)),
            "sockstat" | "sockstat6" => return Some(sockstat(e.data, &socks(), &p.udp_socks(), p.socket_count(), e.name == "sockstat6")),
            "udp" => return Some(udp_table(&p.udp_socks(), false)),
            "udp6" => return Some(udp_table(&p.udp_socks(), true)),
            "unix" => return Some(unix_table(&p.unix_socks())),
            _ => {}
        }
    }
    Some(e.data.to_vec())
}

/// Endereço como o `%08X` do kernel imprime: cada palavra de 32 bits lida na ordem do host.
fn addr_hex(ip: &[u8; 16], words: usize) -> String {
    ip.chunks(4).take(words).map(|w| format!("{:08X}", u32::from_le_bytes([w[0], w[1], w[2], w[3]]))).collect()
}

/// `tcp4_seq_show`/`tcp6_seq_show`. No IPv4 o cabeçalho e cada linha vão até 149 colunas
/// (`seq_setwidth(seq, TMPSZ - 1)` com `seq_pad`); o IPv6 não preenche.
fn tcp_table(socks: &[TcpSock], v6: bool) -> Vec<u8> {
    const WIDTH: usize = 149;
    let mut out = String::new();
    if v6 {
        out.push_str("  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n");
    } else {
        out.push_str(&format!("{:<WIDTH$}\n", "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode"));
    }
    let words = if v6 { 4 } else { 1 };
    for (sl, s) in socks.iter().filter(|s| s.v6 == v6).enumerate() {
        let mut line = format!(
            "{sl:4}: {}:{:04X} {}:{:04X} {:02X} {:08X}:{:08X} {:02X}:{:08X} {:08X} {:5} {:8} {} {} {:016x}",
            addr_hex(&s.local_ip, words),
            s.local_port,
            addr_hex(&s.remote_ip, words),
            s.remote_port,
            s.state,
            s.tx_queue,
            s.rx_queue,
            s.timer,
            s.when,
            s.retrans,
            s.uid,
            0,
            s.inode,
            s.refcnt,
            s.ptr,
        );
        if let Some((rto, ato, qack, cwnd, ssthresh)) = s.tail {
            line.push_str(&format!(" {rto} {ato} {qack} {cwnd} {ssthresh}"));
        }
        if v6 {
            out.push_str(&line);
            out.push('\n');
        } else {
            out.push_str(&format!("{line:<WIDTH$}\n"));
        }
    }
    out.into_bytes()
}

/// O `sockstat` capturado com as contagens de TCP e UDP das tabelas: `inuse` conta os sockets
/// completos (em escuta e conectados) de cada família, `orphan` os já fechados que ainda estão no
/// FIN_WAIT2 e `tw` os de time-wait; no UDP, os sockets com porta, e `mem` soma as páginas das filas
/// ao contador capturado (que no Linux é global, do host inteiro). `sockets: used` soma os sockets do
/// sandbox ao valor capturado.
fn sockstat(base: &[u8], socks: &[TcpSock], udp: &[UdpSockRow], used: usize, v6: bool) -> Vec<u8> {
    let inuse = |fam: bool| socks.iter().filter(|s| s.v6 == fam && s.tail.is_some()).count();
    let tw = socks.iter().filter(|s| s.tail.is_none()).count();
    let udp_inuse = |fam: bool| udp.iter().filter(|s| s.v6 == fam).count();
    let udp_pages: u64 = udp.iter().map(|s| u64::from(s.rx_queue).div_ceil(4096)).sum();
    let field = |line: &[u8], name: &str| {
        std::str::from_utf8(line)
            .ok()
            .and_then(|l| l.split_whitespace().skip_while(|w| *w != name).nth(1))
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let mut out = Vec::new();
    for line in base.split_inclusive(|&b| b == b'\n') {
        if !v6 && line.starts_with(b"sockets: ") {
            out.extend_from_slice(format!("sockets: used {}\n", field(line, "used") + used as u64).as_bytes());
        } else if !v6 && line.starts_with(b"TCP: ") {
            let alloc = field(line, "alloc") as usize;
            out.extend_from_slice(
                format!("TCP: inuse {} orphan 0 tw {tw} alloc {} mem 0\n", inuse(false), alloc + inuse(false) + inuse(true)).as_bytes(),
            );
        } else if !v6 && line.starts_with(b"UDP: ") {
            out.extend_from_slice(format!("UDP: inuse {} mem {}\n", udp_inuse(false), field(line, "mem") + udp_pages).as_bytes());
        } else if v6 && line.starts_with(b"TCP6: ") {
            out.extend_from_slice(format!("TCP6: inuse {}\n", inuse(true)).as_bytes());
        } else if v6 && line.starts_with(b"UDP6: ") {
            out.extend_from_slice(format!("UDP6: inuse {}\n", udp_inuse(true)).as_bytes());
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

/// `udp4_seq_show`/`udp6_seq_show`. No IPv4 o cabeçalho e cada linha vão até 127 colunas
/// (`seq_setwidth(seq, 127)`); o IPv6 não preenche.
fn udp_table(socks: &[UdpSockRow], v6: bool) -> Vec<u8> {
    const WIDTH: usize = 127;
    let mut out = String::new();
    if v6 {
        out.push_str("  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops\n");
    } else {
        out.push_str(&format!("{:<WIDTH$}\n", "   sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops"));
    }
    let words = if v6 { 4 } else { 1 };
    for s in socks.iter().filter(|s| s.v6 == v6) {
        let line = format!(
            "{:5}: {}:{:04X} {}:{:04X} {:02X} {:08X}:{:08X} {:02X}:{:08X} {:08X} {:5} {:8} {} {} {:016x} {}",
            s.sl,
            addr_hex(&s.local_ip, words),
            s.local_port,
            addr_hex(&s.remote_ip, words),
            s.remote_port,
            s.state,
            0,
            s.rx_queue,
            0,
            0,
            0,
            s.uid,
            0,
            s.inode,
            s.refcnt,
            s.ptr,
            s.drops,
        );
        if v6 {
            out.push_str(&line);
            out.push('\n');
        } else {
            out.push_str(&format!("{line:<WIDTH$}\n"));
        }
    }
    out.into_bytes()
}

/// `unix_seq_show`: no espaço abstrato o nulo inicial (e qualquer outro) sai como `@`.
fn unix_table(socks: &[UnixSockRow]) -> Vec<u8> {
    let mut out = b"Num       RefCount Protocol Flags    Type St Inode Path\n".to_vec();
    for s in socks {
        out.extend_from_slice(
            format!("{:016x}: {:08X} {:08X} {:08X} {:04X} {:02X} {:5}", s.ptr, s.refcnt, 0, s.flags, s.ty, s.state, s.inode).as_bytes(),
        );
        if let Some(path) = &s.path {
            out.push(b' ');
            out.extend(path.iter().map(|&b| if b == 0 { b'@' } else { b }));
        }
        out.push(b'\n');
    }
    out
}
