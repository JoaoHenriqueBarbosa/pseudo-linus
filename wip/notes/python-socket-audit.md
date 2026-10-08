# Auditoria do módulo socket (Python 3.13)

Escopo: `crates/ul-python/src/modules/py/_socket.py` lido contra `Modules/socketmodule.c` do CPython 3.13
e o glibc 2.41. Casos para o golden em `testbench/corpus/cases/python/socket_audit.toml`.
Nada foi compilado nem medido: o que segue é leitura e memória do fonte, e o golden decide.

## Corrigido por leitura

- `repr(_socket.socket())` é `<socket object, fd=N, family=N, type=N, proto=N>`; o do `socket.socket` vem do disco.
- `herror` e `gaierror` com `__module__ == 'socket'`; classe morta `error` removida.
- `settimeout`/`setdefaulttimeout` pelo `pytime` (arredonda para cima em ns, NaN, estouro, mensagem de tipo).
- `settimeout` guarda o prazo antes de o fd falhar (socket fechado), como o `sock_settimeout`.
- Mensagens de endereço com o chamador (`bind(): ...`, `connect(): ...`, `sendto(): ...`), tupla de IPv6 de 2 a 4,
  `flowinfo`, porta com `__index__`, host `bytes`/`bytearray`, resolução antes do teste de porta.
- Argumentos antes do `EBADF` (`bind`, `connect`, `send`, `sendto`), `send*` com `_as_bytes` (acabou o `bytes(5)`
  e o `bytes('x')`), `recv*` com `__index__`, `recv_into` e `recvfrom_into` com buffer de escrita.
- `getaddrinfo` reescrito pelo glibc: tabela de tipos (STREAM, DGRAM, RAW sem serviço), serviços do `/etc/services`,
  `EAI_*` e mensagens do `gai_strerror`, `AI_*` inválidos, família, `AI_V4MAPPED`, `AI_CANONNAME` só no primeiro item,
  IPv6 antes de IPv4, `None` com `AI_PASSIVE`, host vazio, nome da máquina e `.invalid` (agora `EAI_AGAIN`, sem rede).
- `inet_aton` com as formas do glibc (`127.1`, octal, hexadecimal), `inet_pton` estrito, `inet_ntop` do IPv6 no
  formato do glibc (IPv4 encapsulado), `htons`/`ntohs`/`htonl`/`ntohl` com mensagens separadas.
- `getservbyname`, `getservbyport`, `getprotobyname` e `getnameinfo` leem `/etc/services`, `/etc/protocols`, `/etc/hosts`.
- `gethostbyaddr` sem o atalho do `127.*`; desconhecido é `herror(2, 'Host name lookup failure')`.
- `if_nametoindex` (`no interface with this name`) e `if_indextoname` (`ENXIO`).
- Constantes que faltavam (AF_*, ALG_*, BTPROTO_*, CAN_*, J1939_*, IP_*, IPV6_*, IPPROTO_*, TCP_*, TIPC_*, VM*,
  NETLINK_*, PACKET_*, EAI_ADDRFAMILY/NODATA, NI_IDN, `CAPI`) e `sendmsg_afalg`.

## Exige medição no oráculo (o golden confirma ou derruba)

1. Mensagens de `htons`/`ntohs` negativo ou acima de 16 bits e de `htonl`/`ntohl` (`int larger than 32 bits`,
   `expected int, str found`): escritas de memória do fonte; o enunciado cita `to C unsigned short`.
2. Texto do `TypeError` de `inet_aton('x')` com tipo errado (`must be str, not int`) e de `if_nametoindex(5)`.
3. Prefixo da mensagem de porta (`bind(): port must be 0-65535.` com o nome do chamador, contra `getsockaddrarg: ...`).
4. `getaddrinfo` com `AI_ADDRCONFIG` numa máquina só com loopback (hoje ignorado), e o item `SOCK_RAW` no caso
   `socktype=0` com porta numérica.
5. `gethostbyaddr` de IP fora do `/etc/hosts` (`TRY_AGAIN` assumido) e `getnameinfo` com `NI_NAMEREQD` (`EAI_AGAIN`).
6. `localhost.` (ponto final) no `/etc/hosts`, hoje casa com `localhost`; o glibc pode mandar ao DNS.
7. `ALG_SET_PUBKEY` (6), `VMADDR_CID_ANY` (0xffffffff) e demais constantes dependentes de cabeçalho do Debian.
8. `socket.socket(type=0)` e `SOCK_RAW` em `AF_INET` (EPERM contra ESOCKTNOSUPPORT), `AF_PACKET` sem privilégio.
9. `getservbyname()` sem argumento, e demais mensagens de aridade dos `METH_VARARGS` (hoje as do interpretador).

## Vazamento da superfície da instância (corrigido pela raiz)

- O VM agora tem `__slots__` fiel: `member_descriptor` por nome (`Classe.x`, `Classe.__dict__`), `__dict__` e
  `__weakref__` só quando o `__slots__` os nomeia, `vars()` e `__dict__` sem dicionário, e o `AttributeError` do 3.13
  (`... and no __dict__ for setting new attributes`).
- `_socket.socket` declara `__slots__` com o estado; o VM esconde o `__slots__` e os nomes dele de `dir`, `vars` e
  `Classe.__dict__` (shim de tipo em C), e `dir(_socket.socket)` vem da tabela `builtin-type-dir.tsv` (fonte: o
  `sock_methods`, `sock_memberlist`, `sock_getsetlist` e `sock_slots` do `socketmodule.c` 3.13; heap type, com `__del__`).
- Resta: os auxiliares privados do shim (`_call`, `_checked`, o próprio `_fd`) ainda respondem a `getattr`, porque o
  shim os usa por `self.`; só `dir`, `vars` e `__dict__` os escondem.

## Medido no oráculo (itens que a leitura errou)

- O `/etc/hosts` numa busca IPv4 lê `::1` como `127.0.0.1` (`localhost` sai duas vezes, `ip6-localhost` resolve em IPv4);
  `gethostbyname_ex` soma apelidos e endereços das linhas do mesmo nome; `localhost.` não casa; `-1` como serviço é `EAI_SERVICE`.
- `AI_PASSIVE` sem nome lista IPv4 antes de IPv6; `bind` aceita `255.255.255.255` (UDP, `RTN_BROADCAST`).
- `inet_aton` recusa valor acima de 32 bits; `inet_aton(x)` com tipo errado usa a mensagem do Clinic (`argument must be`).
- `gethostbyname*`/`gethostbyaddr` com tipo errado usam o formato `et` (`argument 1 must be str, bytes or bytearray`).
- `settimeout`: sem "or float" na mensagem de tipo; `inf` é `timestamp out of range for platform time_t`.
- `sendto` converte o endereço também num socket TCP; `del` de slot vazio diz só o nome; `TIPC_WAIT_FOREVER` é -1.
- `__mro__` de classe com base embutida inclui a cadeia embutida; `__dict__`/`dir` da instância não mostram campos de `__slots__`.

## Fora do alcance desta leitura

- `ResourceWarning` de socket não fechado (`sock_finalize`) não é emitido.
