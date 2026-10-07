//! `/proc/<pid>/net` (e `/proc/net`, que aponta para `self/net`): o namespace de rede de um container
//! sem rede, só com o `lo`. O conteúdo foi capturado do oráculo (`docker run --network none`); o
//! `softnet_stat` tem uma linha por CPU e é cortado no número de CPUs da máquina.

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

/// Conteúdo de um arquivo.
pub(super) fn content(i: u32, ncpus: u32) -> Option<Vec<u8>> {
    let e = ent(i).filter(|e| !e.dir)?;
    if e.parent == 0 && e.name == SOFTNET_STAT {
        let mut out = Vec::new();
        for l in e.data.split_inclusive(|&b| b == b'\n').take(ncpus.max(1) as usize) {
            out.extend_from_slice(l);
        }
        return Some(out);
    }
    Some(e.data.to_vec())
}
