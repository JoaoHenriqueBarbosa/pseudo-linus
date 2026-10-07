//! Árvore rubro-negra aumentada em arena, feita à mão.
//!
//! É uma tradução do `lib/rbtree.c` e do `include/linux/rbtree_augmented.h` do Linux 6.12, com duas
//! diferenças de representação e nenhuma de algoritmo:
//!
//! - Os nós vivem numa arena `Vec<Node>` e se referenciam por índice `u32`. O "ponteiro nulo" do kernel
//!   vira um índice sentinela (`NIL = u32::MAX`). Isso dispensa ponteiros crus, `Rc<RefCell>` e `unsafe`,
//!   e deixa os nós contíguos na memória. Nós removidos vão pra uma lista de livres e são reaproveitados.
//! - O kernel guarda pai e cor no mesmo `unsigned long` (`__rb_parent_color`); aqui são dois campos.
//!
//! Os casos de inserção (`__rb_insert`), remoção (`__rb_erase_augmented`) e rebalanceamento da remoção
//! (`____rb_erase_color`) seguem o código do kernel linha a linha, inclusive a ordem em que os callbacks
//! de augmentação são chamados (`propagate`, `copy`, `rotate`). O cache do nó mais à esquerda segue o
//! `rb_root_cached` (`rb_add_augmented_cached` e `rb_erase_augmented_cached`).
//!
//! # Chaves repetidas
//!
//! Como no `rb_add_augmented_cached`, a inserção desce pela esquerda só quando a chave nova é
//! estritamente menor que a do nó; chave igual vai pra direita. Chaves repetidas são permitidas e ficam,
//! na ordem simétrica, na ordem em que foram inseridas (rotações preservam a ordem simétrica). É isso que
//! dá ao escalonador o desempate "primeiro a chegar" entre deadlines iguais, igual ao kernel.
//!
//! # Augmentação
//!
//! Cada nó guarda um resumo da sua subárvore, definido pela trait [`Augment`]. O resumo de um nó é função
//! só da chave, do valor e dos resumos dos filhos, e é recalculado em toda inserção, remoção e rotação,
//! exatamente nos pontos em que o kernel chama os callbacks de `RB_DECLARE_CALLBACKS`. É o mecanismo do
//! `min_vruntime` do EEVDF: com ele, o `pick_eevdf` desce a árvore em O(log n) usando
//! [`RbTree::root`], [`RbTree::left`], [`RbTree::right`], [`RbTree::summary`] e [`RbTree::value`].
//!
//! # Handles
//!
//! [`RbTree::insert`] devolve um [`NodeId`], e a remoção é por handle (como o `rb_erase` do kernel, que
//! recebe o nó, não a chave). Um handle vale até o nó ser removido; depois disso o índice pode ser
//! reaproveitado por outra inserção, então quem guarda handles precisa esquecê-los na remoção (o
//! escalonador faz isso, como o kernel faz com `se->run_node`).

use std::fmt;
use std::marker::PhantomData;

/// Índice sentinela que representa "nenhum nó" (o `NULL` do kernel).
const NIL: u32 = u32::MAX;

/// Handle de um nó da árvore: o índice dele na arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    /// Índice do nó na arena. Útil pra indexar tabelas paralelas; não muda enquanto o nó existir.
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Converte índice interno em handle público, com `NIL` virando `None`.
#[inline]
fn opt(i: u32) -> Option<NodeId> {
    if i == NIL { None } else { Some(NodeId(i)) }
}

/// Pânico de handle inválido, fora do caminho quente.
#[cold]
#[inline(never)]
fn not_occupied(i: u32) -> ! {
    panic!("rbtree: handle {i} não aponta pra nó ocupado")
}

/// Define o resumo de subárvore que cada nó carrega (o `RBAUGMENTED` do kernel).
///
/// `summarize` corresponde ao `RBCOMPUTE`: recebe a chave e o valor do nó e os resumos dos filhos
/// (`None` quando o filho não existe) e devolve o resumo da subárvore. A árvore só chama essa função;
/// quem implementa garante que o resultado dependa apenas desses argumentos.
pub trait Augment<K, V> {
    /// Resumo guardado em cada nó.
    type Summary: Clone + PartialEq + fmt::Debug;

    /// Calcula o resumo de um nó a partir dele e dos resumos dos filhos.
    fn summarize(key: &K, value: &V, left: Option<&Self::Summary>, right: Option<&Self::Summary>) -> Self::Summary;
}

/// Augmentação vazia: árvore rubro-negra comum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoAugment;

impl<K, V> Augment<K, V> for NoAugment {
    type Summary = ();

    #[inline]
    fn summarize(_key: &K, _value: &V, _left: Option<&()>, _right: Option<&()>) {}
}

/// Conteúdo de um nó ocupado.
#[derive(Clone, Debug)]
struct Entry<K, V, S> {
    key: K,
    value: V,
    summary: S,
}

/// Nó da arena. Em nó livre, `slot` é `None` e `right` aponta pro próximo livre.
#[derive(Clone, Debug)]
struct Node<K, V, S> {
    parent: u32,
    left: u32,
    right: u32,
    red: bool,
    slot: Option<Entry<K, V, S>>,
}

/// Árvore rubro-negra aumentada, com chave `K`, valor `V` e augmentação `A`.
pub struct RbTree<K, V, A: Augment<K, V> = NoAugment> {
    nodes: Vec<Node<K, V, A::Summary>>,
    root: u32,
    leftmost: u32,
    free_head: u32,
    len: usize,
    augment: PhantomData<fn() -> A>,
}

impl<K, V, A: Augment<K, V>> Default for RbTree<K, V, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone, V: Clone, A: Augment<K, V>> Clone for RbTree<K, V, A> {
    fn clone(&self) -> Self {
        RbTree {
            nodes: self.nodes.clone(),
            root: self.root,
            leftmost: self.leftmost,
            free_head: self.free_head,
            len: self.len,
            augment: PhantomData,
        }
    }
}

impl<K: fmt::Debug, V: fmt::Debug, A: Augment<K, V>> fmt::Debug for RbTree<K, V, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter().map(|(_, k, v)| (k, v))).finish()
    }
}

impl<K, V, A: Augment<K, V>> RbTree<K, V, A> {
    /// Árvore vazia.
    pub const fn new() -> Self {
        RbTree { nodes: Vec::new(), root: NIL, leftmost: NIL, free_head: NIL, len: 0, augment: PhantomData }
    }

    /// Árvore vazia com espaço reservado pra `capacity` nós.
    pub fn with_capacity(capacity: usize) -> Self {
        RbTree {
            nodes: Vec::with_capacity(capacity),
            root: NIL,
            leftmost: NIL,
            free_head: NIL,
            len: 0,
            augment: PhantomData,
        }
    }

    /// Número de nós na árvore.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Diz se a árvore está vazia.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Remove tudo e libera a arena.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = NIL;
        self.leftmost = NIL;
        self.free_head = NIL;
        self.len = 0;
    }

    // ---------------------------------------------------------------------------------------------
    // Navegação
    // ---------------------------------------------------------------------------------------------

    /// Raiz da árvore (`tasks_timeline.rb_root.rb_node`).
    pub fn root(&self) -> Option<NodeId> {
        opt(self.root)
    }

    /// Nó mais à esquerda, lido do cache em O(1) (`rb_first_cached`).
    pub fn first(&self) -> Option<NodeId> {
        opt(self.leftmost)
    }

    /// Nó mais à direita, em O(log n) (`rb_last`).
    pub fn last(&self) -> Option<NodeId> {
        let mut i = self.root;
        if i == NIL {
            return None;
        }
        while self.node(i).right != NIL {
            i = self.node(i).right;
        }
        Some(NodeId(i))
    }

    /// Sucessor na ordem simétrica (`rb_next`).
    pub fn next(&self, id: NodeId) -> Option<NodeId> {
        self.assert_occupied(id.0);
        opt(self.next_index(id.0))
    }

    /// Antecessor na ordem simétrica (`rb_prev`).
    pub fn prev(&self, id: NodeId) -> Option<NodeId> {
        self.assert_occupied(id.0);
        let mut node = id.0;
        if self.node(node).left != NIL {
            node = self.node(node).left;
            while self.node(node).right != NIL {
                node = self.node(node).right;
            }
            return Some(NodeId(node));
        }
        let mut parent = self.node(node).parent;
        while parent != NIL && node == self.node(parent).left {
            node = parent;
            parent = self.node(node).parent;
        }
        opt(parent)
    }

    /// Filho esquerdo.
    pub fn left(&self, id: NodeId) -> Option<NodeId> {
        self.assert_occupied(id.0);
        opt(self.node(id.0).left)
    }

    /// Filho direito.
    pub fn right(&self, id: NodeId) -> Option<NodeId> {
        self.assert_occupied(id.0);
        opt(self.node(id.0).right)
    }

    /// Pai.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.assert_occupied(id.0);
        opt(self.node(id.0).parent)
    }

    /// Diz se o nó é vermelho.
    pub fn is_red(&self, id: NodeId) -> bool {
        self.assert_occupied(id.0);
        self.node(id.0).red
    }

    /// Chave do nó.
    pub fn key(&self, id: NodeId) -> &K {
        &self.entry(id.0).key
    }

    /// Valor do nó.
    pub fn value(&self, id: NodeId) -> &V {
        &self.entry(id.0).value
    }

    /// Resumo da subárvore com raiz no nó.
    pub fn summary(&self, id: NodeId) -> &A::Summary {
        &self.entry(id.0).summary
    }

    /// Chave e valor, ou `None` se o handle não aponta pra nó ocupado.
    pub fn get(&self, id: NodeId) -> Option<(&K, &V)> {
        self.nodes.get(id.0 as usize).and_then(|n| n.slot.as_ref()).map(|e| (&e.key, &e.value))
    }

    /// Diz se o handle aponta pra nó ocupado.
    pub fn contains(&self, id: NodeId) -> bool {
        self.get(id).is_some()
    }

    /// Percorre a árvore em ordem simétrica.
    pub fn iter(&self) -> Iter<'_, K, V, A> {
        Iter { tree: self, next: self.leftmost, remaining: self.len }
    }

    // ---------------------------------------------------------------------------------------------
    // Mutação que não depende de ordem
    // ---------------------------------------------------------------------------------------------

    /// Altera o valor de um nó e recalcula os resumos do caminho até a raiz.
    ///
    /// Equivale a mexer no campo do nó e chamar `RBNAME_propagate(node, NULL)` (no fair.c,
    /// `min_vruntime_cb_propagate`). A propagação para no primeiro ancestral cujo resumo não muda.
    /// A chave não pode ser alterada assim: pra mudar a chave, remova e insira de novo.
    pub fn update_value<R>(&mut self, id: NodeId, f: impl FnOnce(&mut V) -> R) -> R {
        let out = f(&mut self.entry_mut(id.0).value);
        self.propagate(id.0, NIL);
        out
    }

    /// Remove o nó e devolve chave e valor (`rb_erase_augmented_cached`).
    ///
    /// Entra em pânico se o handle não aponta pra nó ocupado.
    pub fn remove(&mut self, id: NodeId) -> (K, V) {
        let node = id.0;
        self.assert_occupied(node);
        if self.leftmost == node {
            self.leftmost = self.next_index(node);
        }
        let rebalance = self.erase_augmented(node);
        if rebalance != NIL {
            self.erase_color(rebalance);
        }
        self.len -= 1;
        let entry = self.release(node);
        (entry.key, entry.value)
    }

    // ---------------------------------------------------------------------------------------------
    // Acesso interno à arena
    // ---------------------------------------------------------------------------------------------

    #[inline]
    fn node(&self, i: u32) -> &Node<K, V, A::Summary> {
        &self.nodes[i as usize]
    }

    #[inline]
    fn node_mut(&mut self, i: u32) -> &mut Node<K, V, A::Summary> {
        &mut self.nodes[i as usize]
    }

    #[inline]
    fn entry(&self, i: u32) -> &Entry<K, V, A::Summary> {
        match self.nodes.get(i as usize).and_then(|n| n.slot.as_ref()) {
            Some(e) => e,
            None => not_occupied(i),
        }
    }

    #[inline]
    fn entry_mut(&mut self, i: u32) -> &mut Entry<K, V, A::Summary> {
        match self.nodes.get_mut(i as usize).and_then(|n| n.slot.as_mut()) {
            Some(e) => e,
            None => not_occupied(i),
        }
    }

    #[inline]
    fn assert_occupied(&self, i: u32) {
        let _ = self.entry(i);
    }

    /// Pega uma posição da lista de livres (ou cresce a arena) e monta um nó vermelho.
    fn alloc(&mut self, parent: u32, entry: Entry<K, V, A::Summary>) -> u32 {
        let node = Node { parent, left: NIL, right: NIL, red: true, slot: Some(entry) };
        if self.free_head != NIL {
            let i = self.free_head;
            self.free_head = self.node(i).right;
            *self.node_mut(i) = node;
            i
        } else {
            let i = u32::try_from(self.nodes.len()).ok().filter(|&i| i != NIL);
            let Some(i) = i else { panic!("rbtree: arena cheia (mais de {} nós)", NIL - 1) };
            self.nodes.push(node);
            i
        }
    }

    /// Devolve a posição pra lista de livres e entrega o conteúdo.
    fn release(&mut self, i: u32) -> Entry<K, V, A::Summary> {
        let free_head = self.free_head;
        let node = self.node_mut(i);
        let entry = node.slot.take().expect("rbtree: liberação de nó já livre");
        node.parent = NIL;
        node.left = NIL;
        node.right = free_head;
        node.red = false;
        self.free_head = i;
        entry
    }

    /// `rb_next` sobre índices.
    fn next_index(&self, mut node: u32) -> u32 {
        if self.node(node).right != NIL {
            node = self.node(node).right;
            while self.node(node).left != NIL {
                node = self.node(node).left;
            }
            return node;
        }
        let mut parent = self.node(node).parent;
        while parent != NIL && node == self.node(parent).right {
            node = parent;
            parent = self.node(node).parent;
        }
        parent
    }

    #[inline]
    fn is_red_idx(&self, i: u32) -> bool {
        self.node(i).red
    }

    /// `rb_set_parent_color`.
    #[inline]
    fn set_parent_color(&mut self, i: u32, parent: u32, red: bool) {
        let n = self.node_mut(i);
        n.parent = parent;
        n.red = red;
    }

    /// `__rb_change_child`: no pai de `old` (ou na raiz), troca `old` por `new`.
    #[inline]
    fn change_child(&mut self, old: u32, new: u32, parent: u32) {
        if parent != NIL {
            if self.node(parent).left == old {
                self.node_mut(parent).left = new;
            } else {
                self.node_mut(parent).right = new;
            }
        } else {
            self.root = new;
        }
    }

    /// `__rb_rotate_set_parents`: `new` herda pai e cor de `old`; `old` ganha `new` como pai e a cor
    /// pedida.
    #[inline]
    fn rotate_set_parents(&mut self, old: u32, new: u32, red: bool) {
        let parent = self.node(old).parent;
        let old_red = self.node(old).red;
        self.set_parent_color(new, parent, old_red);
        self.set_parent_color(old, new, red);
        self.change_child(old, new, parent);
    }

    // ---------------------------------------------------------------------------------------------
    // Augmentação (os três callbacks de RB_DECLARE_CALLBACKS)
    // ---------------------------------------------------------------------------------------------

    /// `RBCOMPUTE`: recalcula o resumo do nó a partir dele e dos filhos. Devolve `true` se o resumo não
    /// mudou (é o valor de retorno que o `propagate` usa pra parar cedo). Resumo de tamanho zero (a
    /// árvore sem augmentação) não tem o que recalcular.
    #[inline]
    fn recompute(&mut self, i: u32) -> bool {
        if std::mem::size_of::<A::Summary>() == 0 {
            return true;
        }
        let node = self.node(i);
        let left = if node.left != NIL { Some(&self.entry(node.left).summary) } else { None };
        let right = if node.right != NIL { Some(&self.entry(node.right).summary) } else { None };
        let entry = self.entry(i);
        let summary = A::summarize(&entry.key, &entry.value, left, right);
        let slot = self.entry_mut(i);
        if slot.summary == summary {
            true
        } else {
            slot.summary = summary;
            false
        }
    }

    /// `RBNAME_propagate(rb, stop)`: sobe recalculando até `stop` ou até um resumo não mudar.
    fn propagate(&mut self, mut i: u32, stop: u32) {
        while i != stop {
            if self.recompute(i) {
                break;
            }
            i = self.node(i).parent;
        }
    }

    /// `RBNAME_copy(old, new)`: `new` passa a ter o resumo de `old`.
    #[inline]
    fn augment_copy(&mut self, old: u32, new: u32) {
        if std::mem::size_of::<A::Summary>() == 0 {
            return;
        }
        let summary = self.entry(old).summary.clone();
        self.entry_mut(new).summary = summary;
    }

    /// `RBNAME_rotate(old, new)`: `new` assume a subárvore inteira que era de `old`, então herda o
    /// resumo dele; `old` desceu e é recalculado.
    fn augment_rotate(&mut self, old: u32, new: u32) {
        self.augment_copy(old, new);
        self.recompute(old);
    }

    // ---------------------------------------------------------------------------------------------
    // Inserção: __rb_insert
    // ---------------------------------------------------------------------------------------------

    /// Rebalanceia depois de ligar o nó vermelho `node` como folha.
    fn insert_color(&mut self, mut node: u32) {
        let mut parent = self.node(node).parent;
        loop {
            // Invariante do laço: `node` é vermelho.
            if parent == NIL {
                // `node` é a raiz: ou é o primeiro nó, ou subimos pelo caso 1 até aqui.
                self.set_parent_color(node, NIL, false);
                break;
            }
            // Pai preto: nada a fazer.
            if !self.is_red_idx(parent) {
                break;
            }
            let gparent = self.node(parent).parent;
            let mut tmp = self.node(gparent).right;
            if parent != tmp {
                // parent == gparent.left
                if tmp != NIL && self.is_red_idx(tmp) {
                    // Caso 1: tio vermelho, inverte cores e continua no avô.
                    self.set_parent_color(tmp, gparent, false);
                    self.set_parent_color(parent, gparent, false);
                    node = gparent;
                    parent = self.node(node).parent;
                    self.set_parent_color(node, parent, true);
                    continue;
                }
                tmp = self.node(parent).right;
                if node == tmp {
                    // Caso 2: tio preto e node é filho direito: rotação à esquerda no pai.
                    tmp = self.node(node).left;
                    self.node_mut(parent).right = tmp;
                    self.node_mut(node).left = parent;
                    if tmp != NIL {
                        self.set_parent_color(tmp, parent, false);
                    }
                    self.set_parent_color(parent, node, true);
                    self.augment_rotate(parent, node);
                    parent = node;
                    tmp = self.node(node).right;
                }
                // Caso 3: tio preto e node é filho esquerdo: rotação à direita no avô.
                self.node_mut(gparent).left = tmp;
                self.node_mut(parent).right = gparent;
                if tmp != NIL {
                    self.set_parent_color(tmp, gparent, false);
                }
                self.rotate_set_parents(gparent, parent, true);
                self.augment_rotate(gparent, parent);
                break;
            } else {
                tmp = self.node(gparent).left;
                if tmp != NIL && self.is_red_idx(tmp) {
                    // Caso 1, espelhado.
                    self.set_parent_color(tmp, gparent, false);
                    self.set_parent_color(parent, gparent, false);
                    node = gparent;
                    parent = self.node(node).parent;
                    self.set_parent_color(node, parent, true);
                    continue;
                }
                tmp = self.node(parent).left;
                if node == tmp {
                    // Caso 2, espelhado: rotação à direita no pai.
                    tmp = self.node(node).right;
                    self.node_mut(parent).left = tmp;
                    self.node_mut(node).right = parent;
                    if tmp != NIL {
                        self.set_parent_color(tmp, parent, false);
                    }
                    self.set_parent_color(parent, node, true);
                    self.augment_rotate(parent, node);
                    parent = node;
                    tmp = self.node(node).left;
                }
                // Caso 3, espelhado: rotação à esquerda no avô.
                self.node_mut(gparent).right = tmp;
                self.node_mut(parent).left = gparent;
                if tmp != NIL {
                    self.set_parent_color(tmp, gparent, false);
                }
                self.rotate_set_parents(gparent, parent, true);
                self.augment_rotate(gparent, parent);
                break;
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Remoção: __rb_erase_augmented e ____rb_erase_color
    // ---------------------------------------------------------------------------------------------

    /// Desliga `node` da árvore, mantendo os resumos certos. Devolve o nó a partir do qual a cor precisa
    /// ser corrigida, ou `NIL` se não precisa.
    fn erase_augmented(&mut self, node: u32) -> u32 {
        let child = self.node(node).right;
        let mut tmp = self.node(node).left;
        let rebalance;

        if tmp == NIL {
            // Caso 1: no máximo um filho (o direito). Se existe, é vermelho e o nó é preto; as cores
            // se ajustam aqui mesmo, sem passar pelo rebalanceamento.
            let pc_parent = self.node(node).parent;
            let pc_red = self.node(node).red;
            let parent = pc_parent;
            self.change_child(node, child, parent);
            if child != NIL {
                self.set_parent_color(child, pc_parent, pc_red);
                rebalance = NIL;
            } else {
                rebalance = if !pc_red { parent } else { NIL };
            }
            tmp = parent;
        } else if child == NIL {
            // Ainda caso 1, com o filho à esquerda.
            let pc_parent = self.node(node).parent;
            let pc_red = self.node(node).red;
            self.set_parent_color(tmp, pc_parent, pc_red);
            let parent = pc_parent;
            self.change_child(node, tmp, parent);
            rebalance = NIL;
            tmp = parent;
        } else {
            let mut successor = child;
            let mut parent;
            let child2;
            tmp = self.node(child).left;
            if tmp == NIL {
                // Caso 2: o sucessor é o próprio filho direito.
                parent = successor;
                child2 = self.node(successor).right;
                self.augment_copy(node, successor);
            } else {
                // Caso 3: o sucessor é o mais à esquerda da subárvore direita.
                loop {
                    parent = successor;
                    successor = tmp;
                    tmp = self.node(tmp).left;
                    if tmp == NIL {
                        break;
                    }
                }
                child2 = self.node(successor).right;
                self.node_mut(parent).left = child2;
                self.node_mut(successor).right = child;
                self.node_mut(child).parent = successor;
                self.augment_copy(node, successor);
                self.propagate(parent, successor);
            }

            tmp = self.node(node).left;
            self.node_mut(successor).left = tmp;
            self.node_mut(tmp).parent = successor;

            let pc_parent = self.node(node).parent;
            let pc_red = self.node(node).red;
            self.change_child(node, successor, pc_parent);

            if child2 != NIL {
                self.set_parent_color(child2, parent, false);
                rebalance = NIL;
            } else {
                rebalance = if !self.is_red_idx(successor) { parent } else { NIL };
            }
            self.set_parent_color(successor, pc_parent, pc_red);
            tmp = successor;
        }

        self.propagate(tmp, NIL);
        rebalance
    }

    /// Corrige a altura preta depois de remover um nó preto sem filhos, começando em `parent`.
    fn erase_color(&mut self, mut parent: u32) {
        let mut node = NIL;
        loop {
            // Invariantes: `node` é preto (ou NIL na primeira volta), não é a raiz, e todo caminho
            // que passa por `parent` e `node` tem um preto a menos que os outros.
            let mut sibling = self.node(parent).right;
            if node != sibling {
                // node == parent.left
                if self.is_red_idx(sibling) {
                    // Caso 1: rotação à esquerda no pai.
                    let tmp1 = self.node(sibling).left;
                    self.node_mut(parent).right = tmp1;
                    self.node_mut(sibling).left = parent;
                    self.set_parent_color(tmp1, parent, false);
                    self.rotate_set_parents(parent, sibling, true);
                    self.augment_rotate(parent, sibling);
                    sibling = tmp1;
                }
                let mut tmp1 = self.node(sibling).right;
                if tmp1 == NIL || !self.is_red_idx(tmp1) {
                    let tmp2 = self.node(sibling).left;
                    if tmp2 == NIL || !self.is_red_idx(tmp2) {
                        // Caso 2: inverte a cor do irmão.
                        self.set_parent_color(sibling, parent, true);
                        if self.is_red_idx(parent) {
                            self.node_mut(parent).red = false;
                        } else {
                            node = parent;
                            parent = self.node(node).parent;
                            if parent != NIL {
                                continue;
                            }
                        }
                        break;
                    }
                    // Caso 3: rotação à direita no irmão. Os pais de `sibling` e `tmp2` só são
                    // acertados no caso 4, como no kernel.
                    tmp1 = self.node(tmp2).right;
                    self.node_mut(sibling).left = tmp1;
                    self.node_mut(tmp2).right = sibling;
                    self.node_mut(parent).right = tmp2;
                    if tmp1 != NIL {
                        self.set_parent_color(tmp1, sibling, false);
                    }
                    self.augment_rotate(sibling, tmp2);
                    tmp1 = sibling;
                    sibling = tmp2;
                }
                // Caso 4: rotação à esquerda no pai e troca de cores.
                let tmp2 = self.node(sibling).left;
                self.node_mut(parent).right = tmp2;
                self.node_mut(sibling).left = parent;
                self.set_parent_color(tmp1, sibling, false);
                if tmp2 != NIL {
                    self.node_mut(tmp2).parent = parent;
                }
                self.rotate_set_parents(parent, sibling, false);
                self.augment_rotate(parent, sibling);
                break;
            } else {
                sibling = self.node(parent).left;
                if self.is_red_idx(sibling) {
                    // Caso 1, espelhado: rotação à direita no pai.
                    let tmp1 = self.node(sibling).right;
                    self.node_mut(parent).left = tmp1;
                    self.node_mut(sibling).right = parent;
                    self.set_parent_color(tmp1, parent, false);
                    self.rotate_set_parents(parent, sibling, true);
                    self.augment_rotate(parent, sibling);
                    sibling = tmp1;
                }
                let mut tmp1 = self.node(sibling).left;
                if tmp1 == NIL || !self.is_red_idx(tmp1) {
                    let tmp2 = self.node(sibling).right;
                    if tmp2 == NIL || !self.is_red_idx(tmp2) {
                        // Caso 2, espelhado.
                        self.set_parent_color(sibling, parent, true);
                        if self.is_red_idx(parent) {
                            self.node_mut(parent).red = false;
                        } else {
                            node = parent;
                            parent = self.node(node).parent;
                            if parent != NIL {
                                continue;
                            }
                        }
                        break;
                    }
                    // Caso 3, espelhado: rotação à esquerda no irmão.
                    tmp1 = self.node(tmp2).left;
                    self.node_mut(sibling).right = tmp1;
                    self.node_mut(tmp2).left = sibling;
                    self.node_mut(parent).left = tmp2;
                    if tmp1 != NIL {
                        self.set_parent_color(tmp1, sibling, false);
                    }
                    self.augment_rotate(sibling, tmp2);
                    tmp1 = sibling;
                    sibling = tmp2;
                }
                // Caso 4, espelhado: rotação à direita no pai e troca de cores.
                let tmp2 = self.node(sibling).right;
                self.node_mut(parent).left = tmp2;
                self.node_mut(sibling).right = parent;
                self.set_parent_color(tmp1, sibling, false);
                if tmp2 != NIL {
                    self.node_mut(tmp2).parent = parent;
                }
                self.rotate_set_parents(parent, sibling, false);
                self.augment_rotate(parent, sibling);
                break;
            }
        }
    }
}

impl<K: Ord, V, A: Augment<K, V>> RbTree<K, V, A> {
    /// Insere e devolve o handle do nó novo (`rb_add_augmented_cached`).
    ///
    /// Desce pela esquerda quando a chave nova é estritamente menor que a do nó, senão pela direita;
    /// por isso chaves iguais ficam na ordem de inserção. O resumo do nó novo nasce como folha, o caminho
    /// até a raiz é recalculado e só então a árvore é rebalanceada, na mesma ordem do kernel.
    pub fn insert(&mut self, key: K, value: V) -> NodeId {
        let mut parent = NIL;
        let mut link_left = false;
        let mut leftmost = true;
        let mut cur = self.root;
        while cur != NIL {
            parent = cur;
            let n = &self.nodes[cur as usize];
            let k = match &n.slot {
                Some(e) => &e.key,
                None => not_occupied(cur),
            };
            if key < *k {
                link_left = true;
                cur = n.left;
            } else {
                link_left = false;
                leftmost = false;
                cur = n.right;
            }
        }
        let summary = A::summarize(&key, &value, None, None);
        let idx = self.alloc(parent, Entry { key, value, summary });
        if parent == NIL {
            self.root = idx;
        } else if link_left {
            self.node_mut(parent).left = idx;
        } else {
            self.node_mut(parent).right = idx;
        }
        self.propagate(parent, NIL);
        if leftmost {
            self.leftmost = idx;
        }
        self.insert_color(idx);
        self.len += 1;
        NodeId(idx)
    }

    /// Primeiro nó (na ordem simétrica) com chave maior ou igual a `key`.
    pub fn lower_bound(&self, key: &K) -> Option<NodeId> {
        let mut cur = self.root;
        let mut best = NIL;
        while cur != NIL {
            let n = &self.nodes[cur as usize];
            let k = match &n.slot {
                Some(e) => &e.key,
                None => not_occupied(cur),
            };
            if *k < *key {
                cur = n.right;
            } else {
                best = cur;
                cur = n.left;
            }
        }
        opt(best)
    }

    /// Primeiro nó (na ordem simétrica) com chave igual a `key`.
    pub fn find(&self, key: &K) -> Option<NodeId> {
        self.lower_bound(key).filter(|&id| self.entry(id.0).key == *key)
    }

    /// Verifica todas as invariantes e devolve um retrato da forma da árvore.
    ///
    /// Confere: raiz preta e sem pai; ligações pai/filho consistentes; nenhum vermelho com filho
    /// vermelho; mesma altura preta em todos os caminhos; ordem simétrica não decrescente; resumo de
    /// cada nó igual ao recalculado a partir dele e dos filhos; cache do mais à esquerda certo; contagem
    /// igual a `len`; lista de livres sem ciclo e cobrindo exatamente as posições vazias da arena.
    pub fn check_invariants(&self) -> Result<InvariantReport, InvariantViolation> {
        if self.root == NIL {
            if self.len != 0 {
                return Err(InvariantViolation::LenMismatch { counted: 0, len: self.len });
            }
            if self.leftmost != NIL {
                return Err(InvariantViolation::WrongLeftmost { expected: None, cached: Some(self.leftmost) });
            }
        } else {
            if self.node(self.root).slot.is_none() {
                return Err(InvariantViolation::FreeSlotInTree { node: self.root });
            }
            if self.node(self.root).parent != NIL {
                return Err(InvariantViolation::RootHasParent);
            }
            if self.node(self.root).red {
                return Err(InvariantViolation::RootNotBlack);
            }
        }
        let shape = self.check_subtree(self.root, NIL)?;
        if shape.count != self.len {
            return Err(InvariantViolation::LenMismatch { counted: shape.count, len: self.len });
        }

        // Ordem simétrica e cache do mais à esquerda. Só é seguro andar com `next_index` depois de saber
        // que as ligações estão consistentes.
        if self.root != NIL {
            let mut first = self.root;
            while self.node(first).left != NIL {
                first = self.node(first).left;
            }
            if first != self.leftmost {
                return Err(InvariantViolation::WrongLeftmost { expected: Some(first), cached: opt(self.leftmost).map(NodeId::index) });
            }
            let mut prev = first;
            let mut cur = self.next_index(first);
            while cur != NIL {
                if self.entry(cur).key < self.entry(prev).key {
                    return Err(InvariantViolation::OrderViolation { node: cur });
                }
                prev = cur;
                cur = self.next_index(cur);
            }
        }

        // Lista de livres.
        let mut free = 0usize;
        let mut cur = self.free_head;
        while cur != NIL {
            if free > self.nodes.len() || self.nodes.get(cur as usize).is_none_or(|n| n.slot.is_some()) {
                return Err(InvariantViolation::FreeListCorrupt);
            }
            free += 1;
            cur = self.node(cur).right;
        }
        if free + self.len != self.nodes.len() {
            return Err(InvariantViolation::FreeListCorrupt);
        }

        Ok(InvariantReport { len: self.len, black_height: shape.black_height, height: shape.height })
    }

    fn check_subtree(&self, i: u32, parent: u32) -> Result<Shape, InvariantViolation> {
        if i == NIL {
            return Ok(Shape { black_height: 1, height: 0, count: 0 });
        }
        let node = match self.nodes.get(i as usize) {
            Some(n) if n.slot.is_some() => n,
            _ => return Err(InvariantViolation::FreeSlotInTree { node: i }),
        };
        if node.parent != parent {
            return Err(InvariantViolation::BrokenParentLink { node: i });
        }
        if node.red && ((node.left != NIL && self.is_red_idx(node.left)) || (node.right != NIL && self.is_red_idx(node.right))) {
            return Err(InvariantViolation::RedRed { node: i });
        }
        let left = self.check_subtree(node.left, i)?;
        let right = self.check_subtree(node.right, i)?;
        if left.black_height != right.black_height {
            return Err(InvariantViolation::BlackHeightMismatch { node: i });
        }
        let entry = self.entry(i);
        let ls = if node.left != NIL { Some(&self.entry(node.left).summary) } else { None };
        let rs = if node.right != NIL { Some(&self.entry(node.right).summary) } else { None };
        if A::summarize(&entry.key, &entry.value, ls, rs) != entry.summary {
            return Err(InvariantViolation::StaleSummary { node: i });
        }
        Ok(Shape {
            black_height: left.black_height + usize::from(!node.red),
            height: 1 + left.height.max(right.height),
            count: 1 + left.count + right.count,
        })
    }
}

struct Shape {
    black_height: usize,
    height: usize,
    count: usize,
}

/// Retrato da forma da árvore, devolvido por [`RbTree::check_invariants`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvariantReport {
    /// Número de nós.
    pub len: usize,
    /// Altura preta, contando a folha nula.
    pub black_height: usize,
    /// Altura em nós do caminho mais longo.
    pub height: usize,
}

/// Invariante violada encontrada por [`RbTree::check_invariants`]. Os números são índices da arena.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvariantViolation {
    RootNotBlack,
    RootHasParent,
    BrokenParentLink { node: u32 },
    RedRed { node: u32 },
    BlackHeightMismatch { node: u32 },
    OrderViolation { node: u32 },
    StaleSummary { node: u32 },
    WrongLeftmost { expected: Option<u32>, cached: Option<u32> },
    LenMismatch { counted: usize, len: usize },
    FreeListCorrupt,
    FreeSlotInTree { node: u32 },
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvariantViolation::RootNotBlack => write!(f, "a raiz é vermelha"),
            InvariantViolation::RootHasParent => write!(f, "a raiz tem pai"),
            InvariantViolation::BrokenParentLink { node } => write!(f, "o nó {node} não aponta pro pai certo"),
            InvariantViolation::RedRed { node } => write!(f, "o nó vermelho {node} tem filho vermelho"),
            InvariantViolation::BlackHeightMismatch { node } => write!(f, "alturas pretas diferentes abaixo do nó {node}"),
            InvariantViolation::OrderViolation { node } => write!(f, "o nó {node} está fora de ordem"),
            InvariantViolation::StaleSummary { node } => write!(f, "o resumo do nó {node} está desatualizado"),
            InvariantViolation::WrongLeftmost { expected, cached } => {
                write!(f, "cache do mais à esquerda errado: esperado {expected:?}, guardado {cached:?}")
            }
            InvariantViolation::LenMismatch { counted, len } => write!(f, "contei {counted} nós mas len é {len}"),
            InvariantViolation::FreeListCorrupt => write!(f, "lista de livres corrompida"),
            InvariantViolation::FreeSlotInTree { node } => write!(f, "posição livre {node} ligada na árvore"),
        }
    }
}

impl std::error::Error for InvariantViolation {}

/// Iterador em ordem simétrica: devolve handle, chave e valor.
pub struct Iter<'a, K, V, A: Augment<K, V>> {
    tree: &'a RbTree<K, V, A>,
    next: u32,
    remaining: usize,
}

impl<K, V, A: Augment<K, V>> fmt::Debug for Iter<'_, K, V, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter").field("next", &opt(self.next)).field("remaining", &self.remaining).finish()
    }
}

impl<'a, K, V, A: Augment<K, V>> Iterator for Iter<'a, K, V, A> {
    type Item = (NodeId, &'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next == NIL {
            return None;
        }
        let i = self.next;
        self.next = self.tree.next_index(i);
        self.remaining -= 1;
        let e = self.tree.entry(i);
        Some((NodeId(i), &e.key, &e.value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<K, V, A: Augment<K, V>> ExactSizeIterator for Iter<'_, K, V, A> {}

impl<'a, K, V, A: Augment<K, V>> IntoIterator for &'a RbTree<K, V, A> {
    type Item = (NodeId, &'a K, &'a V);
    type IntoIter = Iter<'a, K, V, A>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests;
