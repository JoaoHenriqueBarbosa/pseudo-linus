//! Arrays associativos com a ordem de iteração do gawk 5.2.1.
//!
//! Agentes imprimem arrays com `for (k in a)` sem `sort`, então a ordem em que o gawk entrega as chaves
//! (sem `PROCINFO["sorted_in"]`) faz parte do comportamento observável. Este módulo reproduz essa ordem
//! a partir do comportamento medido no gawk real, usado como caixa-preta (nenhum código do gawk foi lido
//! nem copiado).
//!
//! Contrato (não mude as assinaturas sem combinar com o integrador):
//!
//! - [`Subscript`] é o subscrito já convertido: o interpretador converte número inteiro com
//!   [`Subscript::from_int`], número não inteiro (já formatado com `CONVFMT`) com
//!   [`Subscript::from_non_integer`] e o resto (textos, strnums, subscritos com `SUBSEP`) com
//!   [`Subscript::from_bytes`], que decide se o texto conta como inteiro pela regra do gawk (`"1"` e `1`
//!   são o mesmo elemento; `"01"` não é).
//! - Busca, inserção e remoção são O(1) amortizado; [`AwkArray::keys`] devolve as chaves na ordem do
//!   `for-in` do gawk.
//!
//! # O modelo medido
//!
//! **Quem conta como inteiro.** Um subscrito é inteiro quando vale entre -2^31 e 2^31-1 e, se veio de
//! texto, o texto é a forma decimal canônica: só dígitos, com `-` opcional na frente, sem zero à esquerda
//! e sem `"-0"`. `"+1"`, `" 1"`, `"1 "`, `"1.0"`, `"1e3"`, `"01"`, `"-0"` e `"2147483648"` são textos.
//! Número inteiro fora dessa faixa (`2^31`, `2^40`) também vira texto. Número não inteiro é sempre texto,
//! mesmo quando `CONVFMT` o formata como um inteiro (`3.0000001` vira `"3"`); nos sabores cint e int esse
//! `"3"` pode ficar separado do inteiro 3, e então o for-in entrega `3` duas vezes, como o gawk (ver
//! **Busca**).
//!
//! **Três sabores.** O primeiro subscrito inserido num array vazio escolhe o sabor:
//!
//! - inteiro não negativo: sabor *cint*; inteiro negativo: sabor *int*; o resto: sabor *str*;
//! - um sabor guarda o que não aceita num array auxiliar: o *int* põe os textos numa tabela *str*
//!   auxiliar; o *cint* põe tudo o que recusa num auxiliar cujo sabor é escolhido pelo primeiro elemento
//!   recusado (*int* se for inteiro, *str* se não), e esse auxiliar *int* pode ter o seu próprio auxiliar
//!   *str*;
//! - o for-in lista primeiro o auxiliar (recursivamente, auxiliar do auxiliar primeiro) e depois a parte
//!   própria do sabor.
//!
//! **Tabela str.** Hash de 32 bits `h = h * 65599 + c` sobre os bytes, com o byte tomado como `char` com
//! sinal (bytes acima de 127 entram negativos). Balde `h % tamanho`, inserção na cabeça da cadeia, listagem
//! dos baldes em ordem e de cada cadeia a partir da cabeça. Tamanhos: 13, 127, 1021, 8191, 16381, 32749,
//! 65497, 131101, 262147, 524309, 1048583, 2097169, 4194319, 8388617 (medidos) e depois os primos logo
//! acima de 2^24 até 2^30 (extrapolados; só seriam alcançados com mais de 25 milhões de elementos). A
//! tabela cresce quando, contando o elemento que está entrando, `n / tamanho > 2` (divisão inteira); o
//! crescimento acontece antes de ligar o elemento novo e redistribui percorrendo os baldes antigos em ordem,
//! cada cadeia a partir da cabeça, inserindo na cabeça das novas. A busca não reordena nada.
//!
//! **Tabela int.** Hash do inteiro (como `u32`) pela mistura final de 32 bits `k ^= k << 3; k += k >> 5;
//! k ^= k << 4; k += k >> 17; k ^= k << 25; k += k >> 6`, balde `hash % tamanho`, com os mesmos tamanhos e a
//! mesma regra de crescimento da tabela str (contando só os inteiros, não o auxiliar). Cada balde é uma
//! cadeia de nós de dois lugares; só o nó da cabeça pode estar pela metade. Elemento novo completa o nó da
//! cabeça se ele tem um lugar livre, senão abre um nó novo na cabeça. A listagem segue os nós a partir da
//! cabeça e, dentro do nó, o lugar 0 e depois o 1. Na remoção, o lugar 1 desce para o 0 se preciso; se o
//! nó que perdeu o elemento não é a cabeça, o último elemento da cabeça vai para o lugar livre; nó vazio
//! sai da cadeia. O crescimento reinsere os elementos na ordem da listagem antiga.
//!
//! **Parte cint.** Guarda inteiros de 0 a 2^31-1 em blocos por potência de dois: a classe de `k` é 10 para
//! `k < 1024` e `bits(k)` para os demais, e cada classe vira uma árvore de folhas (vetores de 32 a 1024
//! posições) alocadas sob demanda. A listagem é a ordem numérica crescente. A capacidade é a soma dos
//! tamanhos das folhas alocadas (folha que esvazia é liberada). Um inteiro novo só entra na parte cint se
//! `capacidade + estimativa(k) - elementos_cint <= 2048`; senão vai para o auxiliar. A estimativa é 2^li,
//! partindo de `li = max(classe(k) - 1, 10)` e trocando `li` por `(li + 1) / 2` enquanto `li >= 10`; ela
//! coincide com o tamanho real da folha, exceto para `2^19 <= k < 2^21`, em que a estimativa é 32 e a
//! folha real tem 1024. O teste é feito antes de saber se a folha de `k` já existe.
//!
//! **Busca.** No sabor cint, um inteiro não negativo é procurado na parte cint e, se não está lá, no
//! auxiliar (que, se for str, compara o texto: é assim que `a[3.0000001]` seguido de `a[3]` dá um só
//! elemento quando o auxiliar é str e dois quando é int). No sabor int, inteiro só é procurado entre os
//! inteiros e texto só no auxiliar. No sabor str, só o texto conta.
//!
//! **Remoção e volta ao vazio.** Quando a parte própria de um sabor fica vazia e o auxiliar ainda tem
//! elementos, o auxiliar é promovido e passa a ser o array (com o estado interno que já tinha). Auxiliar
//! que esvazia é descartado. Array que esvazia (por `delete a` ou por remoções uma a uma) volta a não ter
//! sabor, e o próximo subscrito escolhe de novo.

use std::collections::HashMap;
use std::rc::Rc;

/// Índice nulo nas listas ligadas e nos vetores de posições.
const NIL: u32 = u32::MAX;

/// Classe mínima da parte cint: todos os inteiros abaixo de 2^NHAT ficam numa única árvore.
const NHAT: u32 = 10;

/// Desperdício máximo (capacidade alocada menos elementos) aceito na parte cint: 2^(NHAT + 1).
const THRESHOLD: usize = 2048;

/// Carga máxima média por balde antes de crescer as tabelas str e int.
const CHAIN_MAX: usize = 2;

/// Sequência de tamanhos das tabelas str e int. Até 8388617 foi medida no gawk; o resto (primos logo
/// acima de 2^24 até 2^30) é extrapolação, só alcançável com mais de 25 milhões de elementos.
const SIZES: [u32; 21] = [
    13, 127, 1021, 8191, 16381, 32749, 65497, 131101, 262147, 524309, 1048583, 2097169, 4194319, 8388617, 16777259,
    33554467, 67108879, 134217757, 268435459, 536870923, 1073741827,
];

/// Subscrito de array já convertido pela regra do gawk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Subscript {
    text: Rc<[u8]>,
    int: Option<i64>,
}

impl Subscript {
    /// Subscrito de um número inteiro.
    ///
    /// Conta como inteiro só entre -2^31 e 2^31-1, como no gawk; fora dessa faixa o subscrito é o texto
    /// decimal do número e [`Subscript::int`] devolve `None` (o gawk o guarda como texto).
    pub fn from_int(i: i64) -> Subscript {
        let int = if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&i) { Some(i) } else { None };
        Subscript { text: Rc::from(i.to_string().into_bytes()), int }
    }

    /// Subscrito de um texto; vira inteiro se o texto for a forma canônica de um inteiro, como no gawk.
    ///
    /// Forma canônica: só dígitos ASCII, com `-` opcional na frente, sem zero à esquerda (`"0"` vale,
    /// `"00"`, `"01"` e `"-0"` não), valor entre -2^31 e 2^31-1. Vale para textos puros e para strnums
    /// (campos, `split`, `getline`): o gawk aplica a mesma regra ao texto deles.
    pub fn from_bytes(text: Rc<[u8]>) -> Subscript {
        let int = canonical_int(&text);
        Subscript { text, int }
    }

    /// Subscrito de um número não inteiro, já formatado com `CONVFMT` pelo interpretador.
    ///
    /// Nunca conta como inteiro, mesmo que o texto pareça um (`3.0000001` com `%.6g` vira `"3"`). Nos
    /// sabores cint e int do gawk esse `"3"` pode ser um elemento à parte do inteiro 3 (o for-in entrega
    /// `3` duas vezes), e este construtor preserva esse comportamento. Número inteiro, mesmo vindo de conta
    /// com ponto flutuante, deve usar [`Subscript::from_int`] (ou [`Subscript::from_bytes`] com o texto,
    /// quando não cabe em `i64`).
    pub fn from_non_integer(text: Rc<[u8]>) -> Subscript {
        Subscript { text, int: None }
    }

    /// O texto do subscrito (é o que o `for-in` atribui à variável de laço).
    pub fn text(&self) -> &Rc<[u8]> {
        &self.text
    }

    /// O valor inteiro, quando o subscrito conta como inteiro para o gawk (faixa de -2^31 a 2^31-1).
    pub fn int(&self) -> Option<i64> {
        self.int
    }

    /// O valor, quando é inteiro não negativo (candidato à parte cint).
    fn uint(&self) -> Option<u32> {
        match self.int {
            Some(i) if i >= 0 => u32::try_from(i).ok(),
            _ => None,
        }
    }

    /// O valor como `i32`, quando é inteiro.
    fn int32(&self) -> Option<i32> {
        self.int.and_then(|i| i32::try_from(i).ok())
    }
}

/// Valor do texto quando ele é a forma decimal canônica de um inteiro de 32 bits com sinal.
fn canonical_int(text: &[u8]) -> Option<i64> {
    let (negative, digits) = match text.split_first()? {
        (&b'-', rest) => (true, rest),
        _ => (false, text),
    };
    let first = *digits.first()?;
    if !digits.iter().all(u8::is_ascii_digit) || digits.len() > 10 {
        return None;
    }
    // "0" sozinho vale; "00", "01", "-0" e "-01" não.
    if first == b'0' && text.len() > 1 {
        return None;
    }
    let magnitude = digits.iter().fold(0i64, |acc, &d| acc * 10 + i64::from(d - b'0'));
    let value = if negative { -magnitude } else { magnitude };
    (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value).then_some(value)
}

/// Hash da tabela str: `h = h * 65599 + c` em 32 bits, com cada byte tomado com sinal.
fn str_hash(text: &[u8]) -> u32 {
    text.iter().fold(0u32, |h, &c| h.wrapping_mul(65599).wrapping_add(i32::from(c as i8) as u32))
}

/// Hash da tabela int: mistura final de 32 bits sobre o inteiro tomado como `u32`.
fn int_hash(k: i32) -> u32 {
    let mut k = k as u32;
    k ^= k << 3;
    k = k.wrapping_add(k >> 5);
    k ^= k << 4;
    k = k.wrapping_add(k >> 17);
    k ^= k << 25;
    k = k.wrapping_add(k >> 6);
    k
}

/// O próximo tamanho de tabela depois de `current`, se houver.
fn next_size(current: usize) -> Option<usize> {
    SIZES.iter().map(|&s| s as usize).find(|&s| s > current)
}

/// Elemento guardado: chave, valor e as ligações da tabela que o contém.
#[derive(Debug)]
struct Slot<V> {
    key: Subscript,
    value: V,
    /// Tabela str: hash do texto.
    hash: u32,
    /// Tabela str: anterior na cadeia.
    prev: u32,
    /// Tabela str: próximo na cadeia. Tabela int: o nó que guarda o elemento.
    next: u32,
}

/// Arena dos elementos, endereçada por índice.
#[derive(Debug)]
struct Slab<V> {
    slots: Vec<Option<Slot<V>>>,
    free: Vec<u32>,
}

impl<V> Slab<V> {
    fn new() -> Self {
        Slab { slots: Vec::new(), free: Vec::new() }
    }

    fn alloc(&mut self, key: Subscript, value: V) -> u32 {
        let slot = Some(Slot { key, value, hash: 0, prev: NIL, next: NIL });
        match self.free.pop() {
            Some(i) => {
                self.slots[i as usize] = slot;
                i
            }
            None => {
                let i = u32::try_from(self.slots.len()).expect("array com mais de 2^32 elementos");
                self.slots.push(slot);
                i
            }
        }
    }

    fn release(&mut self, i: u32) -> Slot<V> {
        let slot = self.slots[i as usize].take().expect("posição de elemento já liberada");
        self.free.push(i);
        slot
    }

    fn get(&self, i: u32) -> &Slot<V> {
        self.slots[i as usize].as_ref().expect("posição de elemento vazia")
    }

    fn get_mut(&mut self, i: u32) -> &mut Slot<V> {
        self.slots[i as usize].as_mut().expect("posição de elemento vazia")
    }
}

/// Tabela de textos com encadeamento (sabor str, e auxiliar dos outros sabores).
#[derive(Debug, Default)]
struct StrTable {
    /// Cabeça de cada balde; vazio enquanto a tabela não foi alocada.
    heads: Vec<u32>,
    count: usize,
    /// A tabela chegou ao último tamanho e não cresce mais.
    maxed: bool,
    /// Índice texto -> elemento, para busca O(1) independente das colisões do hash do gawk.
    index: HashMap<Rc<[u8]>, u32>,
}

impl StrTable {
    fn find(&self, text: &[u8]) -> Option<u32> {
        self.index.get(text).copied()
    }

    fn insert<V>(&mut self, slab: &mut Slab<V>, s: u32) {
        let slot = slab.get_mut(s);
        let hash = str_hash(&slot.key.text);
        slot.hash = hash;
        let text = slot.key.text.clone();
        if self.heads.is_empty() {
            self.grow(slab);
        }
        self.count += 1;
        if !self.maxed && self.count / self.heads.len() > CHAIN_MAX {
            self.grow(slab);
        }
        self.link_head(slab, s, hash);
        self.index.insert(text, s);
    }

    fn link_head<V>(&mut self, slab: &mut Slab<V>, s: u32, hash: u32) {
        let b = hash as usize % self.heads.len();
        let old = self.heads[b];
        let slot = slab.get_mut(s);
        slot.prev = NIL;
        slot.next = old;
        if old != NIL {
            slab.get_mut(old).prev = s;
        }
        self.heads[b] = s;
    }

    fn grow<V>(&mut self, slab: &mut Slab<V>) {
        let Some(size) = next_size(self.heads.len()) else {
            self.maxed = true;
            return;
        };
        let old = std::mem::replace(&mut self.heads, vec![NIL; size]);
        for head in old {
            let mut s = head;
            while s != NIL {
                let slot = slab.get(s);
                let (next, hash) = (slot.next, slot.hash);
                self.link_head(slab, s, hash);
                s = next;
            }
        }
    }

    fn remove<V>(&mut self, slab: &mut Slab<V>, text: &[u8]) -> Option<u32> {
        let s = self.index.remove(text)?;
        let slot = slab.get(s);
        let (prev, next, hash) = (slot.prev, slot.next, slot.hash);
        if prev == NIL {
            let b = hash as usize % self.heads.len();
            self.heads[b] = next;
        } else {
            slab.get_mut(prev).next = next;
        }
        if next != NIL {
            slab.get_mut(next).prev = prev;
        }
        self.count -= 1;
        Some(s)
    }

    fn list<V>(&self, slab: &Slab<V>, out: &mut Vec<u32>) {
        for &head in &self.heads {
            let mut s = head;
            while s != NIL {
                out.push(s);
                s = slab.get(s).next;
            }
        }
    }
}

/// Nó de dois lugares de uma cadeia da tabela int.
#[derive(Clone, Copy, Debug)]
struct IntNode {
    slots: [u32; 2],
    len: u32,
    next: u32,
}

/// Tabela de inteiros com cadeias de nós de dois lugares (sabor int, e auxiliar do sabor cint).
#[derive(Debug, Default)]
struct IntTable {
    /// Nó da cabeça de cada balde; vazio enquanto a tabela não foi alocada.
    heads: Vec<u32>,
    nodes: Vec<IntNode>,
    free_nodes: Vec<u32>,
    /// Quantos inteiros (sem contar o auxiliar).
    count: usize,
    maxed: bool,
    /// Índice inteiro -> elemento, para busca O(1).
    index: HashMap<i32, u32>,
    /// Auxiliar com os subscritos que não são inteiros.
    xn: Option<StrTable>,
}

impl IntTable {
    fn total(&self) -> usize {
        self.count + self.xn.as_ref().map_or(0, |x| x.count)
    }

    fn find(&self, key: &Subscript) -> Option<u32> {
        match key.int32() {
            Some(k) => self.index.get(&k).copied(),
            None => self.xn.as_ref()?.find(&key.text),
        }
    }

    fn insert<V>(&mut self, slab: &mut Slab<V>, s: u32) {
        match slab.get(s).key.int32() {
            Some(k) => self.insert_int(slab, s, k),
            None => self.xn.get_or_insert_with(StrTable::default).insert(slab, s),
        }
    }

    fn insert_int<V>(&mut self, slab: &mut Slab<V>, s: u32, k: i32) {
        if self.heads.is_empty() {
            self.grow(slab);
        }
        self.count += 1;
        if !self.maxed && self.count / self.heads.len() > CHAIN_MAX {
            self.grow(slab);
        }
        self.push(slab, s, k);
        self.index.insert(k, s);
    }

    fn alloc_node(&mut self, node: IntNode) -> u32 {
        match self.free_nodes.pop() {
            Some(i) => {
                self.nodes[i as usize] = node;
                i
            }
            None => {
                let i = u32::try_from(self.nodes.len()).expect("tabela int grande demais");
                self.nodes.push(node);
                i
            }
        }
    }

    /// Liga o elemento no seu balde: completa o nó da cabeça ou abre um nó novo na cabeça.
    fn push<V>(&mut self, slab: &mut Slab<V>, s: u32, k: i32) {
        let b = int_hash(k) as usize % self.heads.len();
        let head = self.heads[b];
        if head != NIL && self.nodes[head as usize].len == 1 {
            let node = &mut self.nodes[head as usize];
            node.slots[1] = s;
            node.len = 2;
            slab.get_mut(s).next = head;
        } else {
            let n = self.alloc_node(IntNode { slots: [s, NIL], len: 1, next: head });
            self.heads[b] = n;
            slab.get_mut(s).next = n;
        }
    }

    fn grow<V>(&mut self, slab: &mut Slab<V>) {
        let Some(size) = next_size(self.heads.len()) else {
            self.maxed = true;
            return;
        };
        let old_heads = std::mem::replace(&mut self.heads, vec![NIL; size]);
        let old_nodes = std::mem::take(&mut self.nodes);
        self.free_nodes.clear();
        for head in old_heads {
            let mut n = head;
            while n != NIL {
                let node = old_nodes[n as usize];
                for &s in &node.slots[..node.len as usize] {
                    let k = slab.get(s).key.int32().expect("elemento da tabela int sem valor inteiro");
                    self.push(slab, s, k);
                }
                n = node.next;
            }
        }
    }

    fn remove<V>(&mut self, slab: &mut Slab<V>, key: &Subscript) -> Option<u32> {
        match key.int32() {
            Some(k) => self.remove_int(slab, k),
            None => {
                let xn = self.xn.as_mut()?;
                let s = xn.remove(slab, &key.text)?;
                if xn.count == 0 {
                    self.xn = None;
                }
                Some(s)
            }
        }
    }

    fn remove_int<V>(&mut self, slab: &mut Slab<V>, k: i32) -> Option<u32> {
        let s = self.index.remove(&k)?;
        let b = int_hash(k) as usize % self.heads.len();
        let ni = slab.get(s).next;
        let node = &mut self.nodes[ni as usize];
        if node.slots[0] == s {
            node.slots[0] = node.slots[1];
        }
        node.len -= 1;
        node.slots[node.len as usize] = NIL;
        if node.len == 0 {
            // Só o nó da cabeça pode ter um elemento só, então só ele pode esvaziar.
            self.heads[b] = node.next;
            self.free_nodes.push(ni);
        } else if self.heads[b] != ni {
            // O nó do meio fica cheio de novo com o último elemento da cabeça.
            let hi = self.heads[b];
            let head = &mut self.nodes[hi as usize];
            head.len -= 1;
            let moved = head.slots[head.len as usize];
            head.slots[head.len as usize] = NIL;
            let (head_empty, head_next) = (head.len == 0, head.next);
            let node = &mut self.nodes[ni as usize];
            node.slots[1] = moved;
            node.len = 2;
            slab.get_mut(moved).next = ni;
            if head_empty {
                self.heads[b] = head_next;
                self.free_nodes.push(hi);
            }
        }
        self.count -= 1;
        Some(s)
    }

    fn list<V>(&self, slab: &Slab<V>, out: &mut Vec<u32>) {
        if let Some(xn) = &self.xn {
            xn.list(slab, out);
        }
        for &head in &self.heads {
            let mut n = head;
            while n != NIL {
                let node = &self.nodes[n as usize];
                out.extend_from_slice(&node.slots[..node.len as usize]);
                n = node.next;
            }
        }
    }
}

/// Auxiliar do sabor cint: o sabor é escolhido pelo primeiro elemento recusado.
#[derive(Debug)]
enum Xn {
    Str(StrTable),
    Int(IntTable),
}

impl Xn {
    fn find(&self, key: &Subscript) -> Option<u32> {
        match self {
            Xn::Str(t) => t.find(&key.text),
            Xn::Int(t) => t.find(key),
        }
    }

    fn total(&self) -> usize {
        match self {
            Xn::Str(t) => t.count,
            Xn::Int(t) => t.total(),
        }
    }

    fn list<V>(&self, slab: &Slab<V>, out: &mut Vec<u32>) {
        match self {
            Xn::Str(t) => t.list(slab, out),
            Xn::Int(t) => t.list(slab, out),
        }
    }
}

/// Nó interno de uma árvore da parte cint: divide o seu intervalo em filhos de 2^shift posições.
#[derive(Debug)]
struct TreeNode {
    base: u32,
    shift: u32,
    /// Os filhos são folhas (e não outros nós).
    leaf_children: bool,
    children: Vec<u32>,
    /// Elementos guardados abaixo deste nó.
    count: u32,
}

/// Folha da parte cint: uma posição por inteiro do seu intervalo.
#[derive(Debug)]
struct Leaf {
    base: u32,
    slots: Vec<u32>,
    count: u32,
}

/// Classe de um inteiro não negativo: 10 abaixo de 1024, senão o número de bits.
fn cint_class(k: u32) -> u32 {
    if k == 0 {
        return NHAT;
    }
    let r = 31 - k.leading_zeros();
    if r < NHAT { NHAT } else { r + 1 }
}

/// Estimativa do tamanho de folha usada no teste de entrada da parte cint.
fn cint_estimate(k: u32) -> usize {
    let m = cint_class(k) - 1;
    let mut li = if m > NHAT { m } else { NHAT };
    while li >= NHAT {
        li = li.div_ceil(2);
    }
    1usize << li
}

/// Parte cint: inteiros de 0 a 2^31-1 em árvores de folhas por classe, listados em ordem crescente.
#[derive(Debug)]
struct CintTable {
    /// Raiz da árvore de cada classe (índices 10 a 31 usados).
    roots: [u32; 32],
    trees: Vec<TreeNode>,
    free_trees: Vec<u32>,
    leaves: Vec<Leaf>,
    free_leaves: Vec<u32>,
    /// Soma dos tamanhos das folhas alocadas.
    capacity: usize,
    /// Elementos na parte cint (sem o auxiliar).
    count: usize,
    xn: Option<Xn>,
}

impl CintTable {
    fn new() -> Self {
        CintTable {
            roots: [NIL; 32],
            trees: Vec::new(),
            free_trees: Vec::new(),
            leaves: Vec::new(),
            free_leaves: Vec::new(),
            capacity: 0,
            count: 0,
            xn: None,
        }
    }

    fn total(&self) -> usize {
        self.count + self.xn.as_ref().map_or(0, Xn::total)
    }

    /// Cria o nó que cobre 2^m inteiros a partir de `base`.
    fn new_tree(&mut self, m: u32, base: u32) -> u32 {
        let n = m.div_ceil(2);
        let children = vec![NIL; 1usize << (m - n)];
        let node = TreeNode { base, shift: n, leaf_children: n <= NHAT, children, count: 0 };
        match self.free_trees.pop() {
            Some(i) => {
                self.trees[i as usize] = node;
                i
            }
            None => {
                self.trees.push(node);
                (self.trees.len() - 1) as u32
            }
        }
    }

    fn new_leaf(&mut self, size_log: u32, base: u32) -> u32 {
        let size = 1usize << size_log;
        self.capacity += size;
        let leaf = Leaf { base, slots: vec![NIL; size], count: 0 };
        match self.free_leaves.pop() {
            Some(i) => {
                self.leaves[i as usize] = leaf;
                i
            }
            None => {
                self.leaves.push(leaf);
                (self.leaves.len() - 1) as u32
            }
        }
    }

    fn find(&self, k: u32) -> Option<u32> {
        let mut t = self.roots[cint_class(k) as usize];
        while t != NIL {
            let node = &self.trees[t as usize];
            let child = node.children[((k - node.base) >> node.shift) as usize];
            if child == NIL {
                return None;
            }
            if node.leaf_children {
                let leaf = &self.leaves[child as usize];
                let s = leaf.slots[(k - leaf.base) as usize];
                return (s != NIL).then_some(s);
            }
            t = child;
        }
        None
    }

    /// Insere um elemento novo, na parte cint ou no auxiliar, pela regra de desperdício do gawk.
    fn insert<V>(&mut self, slab: &mut Slab<V>, s: u32) {
        let key = &slab.get(s).key;
        if let Some(k) = key.uint()
            && self.capacity + cint_estimate(k) - self.count <= THRESHOLD
        {
            self.insert_uint(s, k);
            return;
        }
        let is_int = key.int.is_some();
        let make = || if is_int { Xn::Int(IntTable::default()) } else { Xn::Str(StrTable::default()) };
        match self.xn.get_or_insert_with(make) {
            Xn::Str(t) => t.insert(slab, s),
            Xn::Int(t) => t.insert(slab, s),
        }
    }

    fn insert_uint(&mut self, s: u32, k: u32) {
        let class = cint_class(k);
        let mut t = self.roots[class as usize];
        if t == NIL {
            let (m, base) = if class == NHAT { (NHAT, 0) } else { (class - 1, 1u32 << (class - 1)) };
            t = self.new_tree(m, base);
            self.roots[class as usize] = t;
        }
        loop {
            let node = &mut self.trees[t as usize];
            node.count += 1;
            let i = ((k - node.base) >> node.shift) as usize;
            let child_base = node.base + ((i as u32) << node.shift);
            let (shift, leaf_children, child) = (node.shift, node.leaf_children, node.children[i]);
            if leaf_children {
                let leaf = if child == NIL {
                    let leaf = self.new_leaf(shift, child_base);
                    self.trees[t as usize].children[i] = leaf;
                    leaf
                } else {
                    child
                };
                let leaf = &mut self.leaves[leaf as usize];
                leaf.slots[(k - leaf.base) as usize] = s;
                leaf.count += 1;
                break;
            }
            t = if child == NIL {
                let sub = self.new_tree(shift, child_base);
                self.trees[t as usize].children[i] = sub;
                sub
            } else {
                child
            };
        }
        self.count += 1;
    }

    /// Remove da parte cint; devolve `None` (sem mexer em nada) se `k` não está lá.
    fn remove_uint(&mut self, k: u32) -> Option<u32> {
        let class = cint_class(k) as usize;
        // Caminho até a folha: (nó, índice do filho). São no máximo dois nós, porque a maior classe
        // (m = 30) tem filhos de 2^15 posições, que viram uma subárvore de folhas de 2^8.
        let mut path: [(u32, usize); 2] = [(NIL, 0); 2];
        let mut depth = 0;
        let mut t = self.roots[class];
        let (leaf_idx, offset) = loop {
            if t == NIL {
                return None;
            }
            let node = &self.trees[t as usize];
            let i = ((k - node.base) >> node.shift) as usize;
            path[depth] = (t, i);
            depth += 1;
            let child = node.children[i];
            if child == NIL {
                return None;
            }
            if node.leaf_children {
                let leaf = &self.leaves[child as usize];
                let offset = (k - leaf.base) as usize;
                if leaf.slots[offset] == NIL {
                    return None;
                }
                break (child, offset);
            }
            t = child;
        };
        let leaf = &mut self.leaves[leaf_idx as usize];
        let s = std::mem::replace(&mut leaf.slots[offset], NIL);
        leaf.count -= 1;
        let mut detach = leaf.count == 0;
        if detach {
            self.capacity -= leaf.slots.len();
            leaf.slots = Vec::new();
            self.free_leaves.push(leaf_idx);
        }
        for level in (0..depth).rev() {
            let (t, i) = path[level];
            let node = &mut self.trees[t as usize];
            if detach {
                node.children[i] = NIL;
            }
            node.count -= 1;
            detach = node.count == 0;
            if detach {
                node.children = Vec::new();
                self.free_trees.push(t);
            }
        }
        if detach {
            self.roots[class] = NIL;
        }
        self.count -= 1;
        Some(s)
    }

    fn remove<V>(&mut self, slab: &mut Slab<V>, key: &Subscript) -> Option<u32> {
        if let Some(k) = key.uint()
            && let Some(s) = self.remove_uint(k)
        {
            return Some(s);
        }
        let s = match self.xn.as_mut()? {
            Xn::Str(t) => t.remove(slab, &key.text)?,
            Xn::Int(t) => t.remove(slab, key)?,
        };
        // Auxiliar vazio some; auxiliar int sem inteiros cede o lugar ao seu próprio auxiliar.
        let xn = self.xn.take();
        self.xn = match xn {
            Some(Xn::Str(t)) if t.count == 0 => None,
            Some(Xn::Int(t)) if t.total() == 0 => None,
            Some(Xn::Int(mut t)) if t.count == 0 => t.xn.take().map(Xn::Str),
            other => other,
        };
        Some(s)
    }

    fn list<V>(&self, slab: &Slab<V>, out: &mut Vec<u32>) {
        if let Some(xn) = &self.xn {
            xn.list(slab, out);
        }
        for &root in &self.roots {
            if root != NIL {
                self.list_tree(root, out);
            }
        }
    }

    fn list_tree(&self, t: u32, out: &mut Vec<u32>) {
        let node = &self.trees[t as usize];
        for &child in &node.children {
            if child == NIL {
                continue;
            }
            if node.leaf_children {
                out.extend(self.leaves[child as usize].slots.iter().copied().filter(|&s| s != NIL));
            } else {
                self.list_tree(child, out);
            }
        }
    }
}

/// O array visto pelo sabor atual.
#[derive(Debug, Default)]
enum Table {
    /// Vazio e sem sabor: o próximo subscrito escolhe.
    #[default]
    Null,
    Str(StrTable),
    Int(IntTable),
    Cint(Box<CintTable>),
}

/// Array associativo com a ordem de iteração do gawk.
#[derive(Debug)]
pub struct AwkArray<V> {
    slab: Slab<V>,
    table: Table,
}

impl<V> Default for AwkArray<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> AwkArray<V> {
    pub fn new() -> Self {
        AwkArray { slab: Slab::new(), table: Table::Null }
    }

    pub fn len(&self) -> usize {
        match &self.table {
            Table::Null => 0,
            Table::Str(t) => t.count,
            Table::Int(t) => t.total(),
            Table::Cint(t) => t.total(),
        }
    }

    /// O sabor atual, com os nomes que o gawk mostra no segundo argumento do `typeof`
    /// (`a["array_type"]`): `null`, `str`, `int` ou `cint`.
    pub fn flavor(&self) -> &'static str {
        match &self.table {
            Table::Null => "null",
            Table::Str(_) => "str",
            Table::Int(_) => "int",
            Table::Cint(_) => "cint",
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &Subscript) -> Option<&V> {
        self.find(key).map(|s| &self.slab.get(s).value)
    }

    pub fn get_mut(&mut self, key: &Subscript) -> Option<&mut V> {
        self.find(key).map(|s| &mut self.slab.get_mut(s).value)
    }

    pub fn contains(&self, key: &Subscript) -> bool {
        self.get(key).is_some()
    }

    /// O elemento de `key`, criado com `make` se não existir (é o que `a[k]` faz no gawk).
    pub fn get_or_insert_with(&mut self, key: &Subscript, make: impl FnOnce() -> V) -> &mut V {
        let s = match self.find(key) {
            Some(s) => s,
            None => self.insert_new(key.clone(), make()),
        };
        &mut self.slab.get_mut(s).value
    }

    /// Troca (ou cria) o valor de `key`, devolvendo o antigo.
    pub fn insert(&mut self, key: Subscript, value: V) -> Option<V> {
        match self.find(&key) {
            Some(s) => Some(std::mem::replace(&mut self.slab.get_mut(s).value, value)),
            None => {
                self.insert_new(key, value);
                None
            }
        }
    }

    /// `delete a[k]`.
    pub fn remove(&mut self, key: &Subscript) -> Option<V> {
        let s = match &mut self.table {
            Table::Null => None,
            Table::Str(t) => t.remove(&mut self.slab, &key.text),
            Table::Int(t) => t.remove(&mut self.slab, key),
            Table::Cint(t) => t.remove(&mut self.slab, key),
        }?;
        let slot = self.slab.release(s);
        self.settle();
        Some(slot.value)
    }

    /// `delete a`.
    pub fn clear(&mut self) {
        self.slab = Slab::new();
        self.table = Table::Null;
    }

    /// Chaves na ordem do `for (k in a)` do gawk sem `sorted_in`.
    pub fn keys(&self) -> Vec<Subscript> {
        self.order().into_iter().map(|s| self.slab.get(s).key.clone()).collect()
    }

    /// Pares (chave, valor) na ordem do `for (k in a)` do gawk sem `sorted_in`.
    ///
    /// A ordem é calculada na chamada (como o gawk faz ao entrar no laço), então o iterador é uma
    /// fotografia do array naquele momento.
    pub fn iter(&self) -> impl Iterator<Item = (&Subscript, &V)> + '_ {
        self.order().into_iter().map(|s| {
            let slot = self.slab.get(s);
            (&slot.key, &slot.value)
        })
    }

    /// Posições dos elementos na ordem do for-in.
    fn order(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.len());
        match &self.table {
            Table::Null => {}
            Table::Str(t) => t.list(&self.slab, &mut out),
            Table::Int(t) => t.list(&self.slab, &mut out),
            Table::Cint(t) => t.list(&self.slab, &mut out),
        }
        out
    }

    /// Procura o elemento seguindo o caminho de busca do gawk para o sabor atual.
    fn find(&self, key: &Subscript) -> Option<u32> {
        match &self.table {
            Table::Null => None,
            Table::Str(t) => t.find(&key.text),
            Table::Int(t) => t.find(key),
            Table::Cint(t) => {
                if let Some(k) = key.uint()
                    && let Some(s) = t.find(k)
                {
                    return Some(s);
                }
                t.xn.as_ref()?.find(key)
            }
        }
    }

    /// Insere um elemento que [`AwkArray::find`] não achou.
    fn insert_new(&mut self, key: Subscript, value: V) -> u32 {
        if matches!(self.table, Table::Null) {
            self.table = match key.int {
                Some(i) if i >= 0 => Table::Cint(Box::new(CintTable::new())),
                Some(_) => Table::Int(IntTable::default()),
                None => Table::Str(StrTable::default()),
            };
        }
        let s = self.slab.alloc(key, value);
        match &mut self.table {
            Table::Null => unreachable!("o sabor acabou de ser escolhido"),
            Table::Str(t) => t.insert(&mut self.slab, s),
            Table::Int(t) => t.insert(&mut self.slab, s),
            Table::Cint(t) => t.insert(&mut self.slab, s),
        }
        s
    }

    /// Depois de uma remoção: promove o auxiliar se a parte própria esvaziou, ou volta a não ter sabor.
    fn settle(&mut self) {
        self.table = match std::mem::take(&mut self.table) {
            Table::Str(t) if t.count == 0 => Table::Null,
            Table::Int(mut t) if t.count == 0 => t.xn.take().map_or(Table::Null, Table::Str),
            Table::Cint(mut t) if t.count == 0 => match t.xn.take() {
                None => Table::Null,
                Some(Xn::Str(x)) => Table::Str(x),
                Some(Xn::Int(x)) => Table::Int(x),
            },
            other => other,
        };
        if self.is_empty() {
            // Sem elementos, a arena volta a ser pequena.
            self.slab = Slab::new();
        }
    }
}

/// Casos fixos com a ordem tirada do gawk 5.2.1 do host (`LC_ALL=C.UTF-8`, sem `AWK_HASH`). Rodam sem
/// o gawk; o testador diferencial que gerou e conferiu esses casos fica fora do repositório.
#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Subscript {
        Subscript::from_bytes(Rc::from(text.as_bytes()))
    }

    fn i(v: i64) -> Subscript {
        Subscript::from_int(v)
    }

    /// Número não inteiro cujo texto (`CONVFMT`) é `text`.
    fn f(text: &str) -> Subscript {
        Subscript::from_non_integer(Rc::from(text.as_bytes()))
    }

    fn build(keys: &[Subscript]) -> AwkArray<()> {
        let mut a = AwkArray::new();
        for k in keys {
            a.get_or_insert_with(k, || ());
        }
        a
    }

    fn order<V>(a: &AwkArray<V>) -> String {
        let keys: Vec<String> =
            a.keys().iter().map(|k| String::from_utf8(k.text().to_vec()).expect("chave UTF-8")).collect();
        keys.join(" ")
    }

    fn ints(range: impl IntoIterator<Item = i64>) -> Vec<Subscript> {
        range.into_iter().map(i).collect()
    }

    /// FNV-1a de 64 bits das chaves unidas por `\n`, para comparar ordens grandes.
    fn fingerprint<V>(a: &AwkArray<V>) -> (usize, u64) {
        let keys = a.keys();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for (n, k) in keys.iter().enumerate() {
            let sep: &[u8] = if n == 0 { b"" } else { b"\n" };
            for &c in sep.iter().chain(k.text().iter()) {
                h ^= u64::from(c);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        (keys.len(), h)
    }

    /// Gerador MINSTD, exato em ponto flutuante, reproduzido igual no programa awk de referência.
    fn minstd(x: &mut i64) -> i64 {
        *x = *x * 48271 % 2_147_483_647;
        *x
    }

    #[test]
    fn integer_text_rule() {
        for t in ["0", "1", "7", "-1", "10", "2147483647", "-2147483648", "123456"] {
            assert_eq!(s(t).int(), Some(t.parse().unwrap()), "{t:?} deveria ser inteiro");
        }
        let not_ints = ["", "-", "-0", "00", "01", "-01", "+1", "+0", " 1", "1 ", "1.0", "1.", ".5", "1e3", "0x1A"];
        let out_of_range = ["2147483648", "-2147483649", "4294967296", "9223372036854775808", "99999999999"];
        for t in not_ints.into_iter().chain(out_of_range).chain(["1\u{1c}2", "١"]) {
            assert_eq!(s(t).int(), None, "{t:?} não deveria ser inteiro");
        }
        assert_eq!(i(2147483647).int(), Some(2147483647));
        assert_eq!(i(-2147483648).int(), Some(-2147483648));
        assert_eq!(i(2147483648).int(), None);
        assert_eq!(i(1 << 40).text().as_ref(), b"1099511627776");
        assert_eq!(f("3").int(), None);
        assert_eq!(s("5"), i(5));
        assert_ne!(f("5"), i(5));
    }

    #[test]
    fn string_table_order() {
        let a = build(&["x", "y", "z", "apple", "banana"].map(s));
        assert_eq!(order(&a), "x apple y z banana");
        let letters: Vec<Subscript> = (b'a'..=b'z').map(|c| s(&(c as char).to_string())).collect();
        assert_eq!(order(&build(&letters)), "u h v i w j x k y l z m n a o b p c q d r e s f t g");
        let a = build(&["ação", "é", "日本", "\u{1c}", "1\u{1c}2", "", "€"].map(s));
        let keys = a.keys();
        let got: Vec<&[u8]> = keys.iter().map(|k| k.text().as_ref()).collect();
        let expected: [&[u8]; 7] =
            ["€".as_bytes(), b"", b"\x1c", "é".as_bytes(), "ação".as_bytes(), b"1\x1c2", "日本".as_bytes()];
        assert_eq!(got, expected);
    }

    #[test]
    fn string_table_growth() {
        let keys: Vec<Subscript> = (0..39).map(|n| s(&format!("k{n}"))).collect();
        assert_eq!(
            order(&build(&keys[..38])),
            "k33 k24 k15 k1 k34 k25 k16 k2 k35 k26 k17 k3 k36 k27 k18 k4 k37 k28 k19 k5 k29 k6 k7 k8 k10 k9 k20 \
             k11 k30 k21 k12 k31 k22 k13 k32 k23 k14 k0"
        );
        assert_eq!(
            order(&build(&keys)),
            "k20 k21 k22 k23 k24 k25 k26 k27 k28 k29 k10 k0 k11 k12 k1 k13 k2 k14 k3 k4 k15 k5 k16 k30 k6 k17 \
             k31 k7 k18 k32 k8 k19 k9 k33 k34 k35 k36 k37 k38"
        );
    }

    #[test]
    fn int_table_order() {
        let mut keys = vec![i(-1)];
        keys.extend(ints(0..=10));
        assert_eq!(order(&build(&keys)), "0 7 4 5 10 -1 8 3 6 2 1 9");
        let mut keys = vec![i(-1)];
        keys.extend(ints((0..=10).rev()));
        assert_eq!(order(&build(&keys)), "7 0 5 4 10 -1 8 3 6 2 9 1");
        assert_eq!(
            order(&build(&ints(-40..=40))),
            "-10 0 -22 30 -17 -19 -38 10 31 -12 -7 15 -36 8 34 -21 -25 17 -35 36 11 -16 5 -30 -40 4 -34 -39 19 \
             -18 12 23 -9 2 20 -33 37 -11 38 21 -26 13 39 -28 -2 28 -15 -23 26 35 -6 24 6 -32 9 -4 -37 -20 3 1 \
             -3 -5 25 -13 -24 -14 18 33 -27 29 16 7 22 40 27 32 -29 -1 -31 14 -8"
        );
    }

    #[test]
    fn int_table_removal_refills_nodes() {
        let mut a = build(&ints((1..=30).map(|n| -n)));
        for n in (1..=30).step_by(3) {
            assert!(a.remove(&i(-n)).is_some());
        }
        a.get_or_insert_with(&i(-100), || ());
        a.get_or_insert_with(&i(-101), || ());
        assert_eq!(order(&a), "-100 -101 -14 -6 -9 -26 -12 -18 -8 -30 -20 -17 -21 -3 -23 -2 -15 -11 -29 -24 -27 -5");
    }

    #[test]
    fn cint_order_and_aux() {
        assert_eq!(order(&build(&ints(1..=20))), (1..=20).map(|n| n.to_string()).collect::<Vec<_>>().join(" "));
        assert_eq!(order(&build(&ints((1..=20).rev()))), order(&build(&ints(1..=20))));
        assert_eq!(order(&build(&[i(1), s("x"), s("01"), f("2.5")])), "x 01 2.5 1");
        assert_eq!(order(&build(&[i(-1), i(-2), i(3), s("x"), f("1.5")])), "x 1.5 -1 3 -2");
        assert_eq!(order(&build(&[i(0), i(-1), s("x"), i(1)])), "x -1 0 1");
        let mut keys = ints(0..=10);
        keys.extend([s("x"), i(-5)]);
        assert_eq!(order(&build(&keys)), "x -5 0 1 2 3 4 5 6 7 8 9 10");
        assert_eq!(order(&build(&ints([5, 100000, 3]))), "3 5 100000");
        assert_eq!(order(&build(&ints([5, 2048, 3, 4096, 1048576, 1048577]))), "3 5 2048 4096 1048576 1048577");
        assert_eq!(
            order(&build(&ints([1, 1 << 31, (1 << 31) - 1, 1 << 32, 1 << 53, 1 << 62, i64::MAX - 1023]))),
            "2147483648 4611686018427387904 9007199254740992 9223372036854774784 4294967296 1 2147483647"
        );
    }

    #[test]
    fn cint_waste_threshold() {
        let seq = [
            747782, 2831, 3684, 1554902337, 737990278, 4531, 4804, 1478, 280026, 153148017, 2876, 1505473590, 859921,
            632044,
        ];
        assert_eq!(
            order(&build(&ints(seq[..9].iter().copied()))),
            "280026 1478 2831 3684 4531 4804 747782 737990278 1554902337"
        );
        assert_eq!(
            order(&build(&ints(seq))),
            "1505473590 280026 632044 1478 2831 2876 3684 4531 4804 747782 859921 153148017 737990278 1554902337"
        );
        // Depois de passar do limiar, nem as folhas já alocadas aceitam inteiros novos.
        let mut keys = vec![0];
        keys.extend((1..=4).map(|n| (1 << 19) + 1024 * n));
        keys.extend([1, 31, 32]);
        assert_eq!(order(&build(&ints(keys))), "31 527360 32 528384 1 0 525312 526336");
        // O limiar é exatamente 2048: com 24 enchimentos a oitava folha de 256 entra.
        for (fill, inside) in [(23, false), (24, true)] {
            let mut keys = vec![0];
            keys.extend((0..7).map(|n| (1 << 30) + n * (1 << 24)));
            keys.extend(1..=fill);
            let probe = (1 << 30) + 100 * (1 << 20) + 5;
            keys.push(probe);
            let a = build(&ints(keys));
            let last = a.keys().last().cloned();
            assert_eq!(last == Some(i(probe)), inside, "enchimentos {fill}");
        }
    }

    #[test]
    fn non_integer_numbers_stay_apart() {
        let a = build(&[i(1), f("3"), i(3)]);
        assert_eq!((order(&a), a.len()), ("3 1".into(), 2));
        let mut a = build(&[i(1), i(-1), f("3"), i(3)]);
        assert_eq!((order(&a), a.len()), ("3 -1 1 3".into(), 4));
        assert!(a.remove(&i(3)).is_some());
        assert_eq!(order(&a), "3 -1 1");
        assert!(a.remove(&f("3")).is_some());
        assert_eq!(order(&a), "-1 1");
        let mut a = build(&[i(-1), f("3"), i(3)]);
        assert_eq!(order(&a), "3 -1 3");
        assert!(a.contains(&i(3)) && a.contains(&s("3")) && a.contains(&f("3")));
        assert!(a.remove(&i(3)).is_some());
        assert_eq!(order(&a), "3 -1");
        assert_eq!(order(&build(&[i(1), s("x"), i(-1), f("-1")])), "-1 x 1");
        assert_eq!(order(&build(&[i(1), i(-1), f("-1")])), "-1 -1 1");
        assert_eq!(order(&build(&[i(1), s("x"), i(3), f("3")])), "x 3 1 3");
        assert_eq!(build(&[i(-1), s("2.5"), f("2.5")]).len(), 2);
        assert_eq!(build(&[s("x"), f("3"), i(3)]).len(), 2);
        // `delete a[3]` acha o "3" de texto quando o auxiliar é str, mas não quando é int.
        let mut a = build(&[i(1), f("3")]);
        assert!(a.remove(&i(3)).is_some());
        assert_eq!(order(&a), "1");
        let mut a = build(&[i(1), i(-1), f("3")]);
        assert!(a.remove(&i(3)).is_none());
        assert_eq!(order(&a), "3 -1 1");
    }

    #[test]
    fn emptied_parts_are_promoted_or_reset() {
        let mut a = build(&[i(1), s("h"), s("u")]);
        a.remove(&i(1));
        a.get_or_insert_with(&i(5), || ());
        a.get_or_insert_with(&i(2), || ());
        assert_eq!(order(&a), "u h 5 2");

        let mut a = build(&[i(1), i(-5), s("x")]);
        a.remove(&i(-5));
        a.get_or_insert_with(&i(-7), || ());
        a.get_or_insert_with(&i(-8), || ());
        assert_eq!(order(&a), "x -7 -8 1");

        // O auxiliar int sem inteiros cede o lugar ao seu auxiliar str: os negativos viram texto.
        let mut a = build(&[i(1), i(-5), s("x")]);
        a.remove(&i(-5));
        for n in 1..=30 {
            a.get_or_insert_with(&i(-n), || ());
        }
        assert_eq!(
            order(&a),
            "-22 -13 -23 -14 -24 -15 -25 -16 -1 x -26 -17 -2 -27 -18 -3 -28 -19 -4 -29 -5 -6 -7 -10 -8 -20 -11 -9 \
             -30 -21 -12 1"
        );

        let mut a = build(&[i(1), i(-5), s("x")]);
        a.remove(&i(1));
        a.get_or_insert_with(&i(7), || ());
        a.get_or_insert_with(&i(8), || ());
        assert_eq!(order(&a), "x 7 8 -5");

        let mut a = build(&[i(-5), s("x"), s("y")]);
        a.remove(&i(-5));
        a.get_or_insert_with(&i(-7), || ());
        a.get_or_insert_with(&i(3), || ());
        assert_eq!(order(&a), "x y -7 3");

        let tail = [i(1), i(3), i(2), i(-1), s("y")];
        let mut a = build(&[s("x")]);
        a.remove(&s("x"));
        for k in &tail {
            a.get_or_insert_with(k, || ());
        }
        assert_eq!(order(&a), "y -1 1 2 3");
        let mut a = build(&[s("x"), i(1), i(3)]);
        a.clear();
        assert!(a.is_empty());
        for k in &tail {
            a.get_or_insert_with(k, || ());
        }
        assert_eq!(order(&a), "y -1 1 2 3");
    }

    #[test]
    fn values_follow_their_keys() {
        let mut a: AwkArray<i32> = AwkArray::new();
        assert_eq!(a.insert(i(1), 10), None);
        assert_eq!(a.insert(s("1"), 11), Some(10));
        *a.get_or_insert_with(&s("x"), || 0) += 5;
        *a.get_or_insert_with(&s("x"), || 0) += 5;
        *a.get_mut(&i(1)).unwrap() += 1;
        assert_eq!(a.get(&s("x")), Some(&10));
        assert_eq!(a.get(&i(1)), Some(&12));
        assert_eq!(a.get(&s("01")), None);
        let pairs: Vec<(String, i32)> =
            a.iter().map(|(k, v)| (String::from_utf8(k.text().to_vec()).unwrap(), *v)).collect();
        assert_eq!(pairs, [("x".to_string(), 10), ("1".to_string(), 12)]);
        assert_eq!(a.remove(&s("x")), Some(10));
        assert_eq!(a.remove(&s("x")), None);
        assert_eq!(a.len(), 1);
        assert_eq!(a.remove(&i(1)), Some(12));
        assert!(a.is_empty());
        assert_eq!(order(&a), "");
    }

    #[test]
    fn large_strings() {
        let a = build(&(0..100_000).map(|n| s(&format!("k{n}"))).collect::<Vec<_>>());
        assert_eq!(fingerprint(&a), (100_000, 0xafbd_4552_a578_c249));
    }

    #[test]
    fn large_negative_ints() {
        let a = build(&ints((1..=100_000).map(|n| -n)));
        assert_eq!(fingerprint(&a), (100_000, 0x61f1_c058_d992_0c9e));
    }

    #[test]
    fn large_dense_ints_with_removals() {
        let mut a = build(&ints(0..100_000));
        for n in (0..100_000).step_by(3) {
            a.remove(&i(n));
        }
        for n in 0..1000 {
            a.get_or_insert_with(&s(&format!("s{n}")), || ());
        }
        assert_eq!(fingerprint(&a), (67_666, 0xe10c_2734_cd29_be82));
    }

    #[test]
    fn large_sparse_ints() {
        let mut x = 1;
        let a = build(&(0..20_000).map(|_| i(minstd(&mut x))).collect::<Vec<_>>());
        assert_eq!(fingerprint(&a), (20_000, 0x628b_924d_2f3b_e313));
    }

    #[test]
    fn large_word_count_with_deletes() {
        let mut a: AwkArray<u32> = AwkArray::new();
        let mut x = 1;
        for _ in 0..50_000 {
            let r = minstd(&mut x);
            let k = s(&format!("w{}", r % 5000));
            if r % 7 == 0 {
                a.remove(&k);
            } else {
                *a.get_or_insert_with(&k, || 0) += 1;
            }
        }
        assert_eq!(fingerprint(&a), (4259, 0xde3e_ef6e_5c83_490c));
    }

    #[test]
    fn large_mixed_flavors() {
        let mut a = AwkArray::new();
        let mut x = 1;
        for _ in 0..60_000 {
            let r = minstd(&mut x);
            let k = match r % 3 {
                0 => i(-(r % 40_000)),
                1 => i(r % 40_000),
                _ => s(&format!("t{}", r % 40_000)),
            };
            if r % 11 == 0 {
                a.remove(&k);
            } else {
                a.get_or_insert_with(&k, || ());
            }
        }
        assert_eq!(fingerprint(&a), (43_080, 0x7381_6b82_9301_e5e7));
    }
}
