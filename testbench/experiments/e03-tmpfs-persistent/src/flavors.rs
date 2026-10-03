//! Os candidatos do experimento.
//!
//! Estrutura (tabela + diretório), todos com o mesmo conteúdo (`Vec<Arc<bloco>>`) pra que a
//! diferença venha só da estrutura:
//!
//! | tipo | tabela de inodes | diretório |
//! |---|---|---|
//! | [`ImblHamt`] | imbl `HashMap` (std `Arc`) | imbl `HashMap` |
//! | [`ImblHamtTriomphe`] | imbl `HashMap` (`triomphe::Arc`) | imbl `HashMap` |
//! | [`ImblBTree`] | imbl `OrdMap` | imbl `OrdMap` |
//! | [`ImHamt`] | im 15 `HashMap` | im 15 `HashMap` |
//! | [`RpdsHamt`] | rpds `HashTrieMapSync` | rpds `HashTrieMapSync` |
//! | [`RpdsRbt`] | rpds `RedBlackTreeMapSync` | rpds `RedBlackTreeMapSync` |
//! | [`ChunkMap`] | immutable-chunkmap `MapM` | immutable-chunkmap `MapM` |
//! | [`HandRadix`] | trie de raiz 64 à mão | `Arc<BTreeMap>` à mão |
//! | [`HandFlat`] | `Arc<BTreeMap>` inteiro (controle negativo) | `Arc<BTreeMap>` à mão |
//!
//! Conteúdo, todos sobre a tabela e o diretório do imbl `HashMap`: [`ContentArcVec`],
//! [`ContentVecBlocks`] (= [`ImblHamt`]), [`ContentImblVector`], [`ContentRpdsVector`],
//! [`ContentRadix`].
//!
//! Combinações finais (o que a recomendação proporia), medidas de ponta a ponta:
//! [`FinalImbl`], [`FinalHand`], [`FinalHandOrd`] e [`FinalImblOrd`].

use archery::{ArcK, ArcTK};

use crate::content::{ArcBytes, Block, Blocks, RadixBlocks};
use crate::maps::{
    BTreeDir, ChunkDir, ChunkTable, FlatTable, ImHashDir, ImHashTable, ImblHashDir, ImblHashTable, ImblOrdDir,
    ImblOrdTable, RadixTable, RpdsHashDir, RpdsHashTable, RpdsRbtDir, RpdsRbtTable, Stack,
};

pub type VecBlocks = Blocks<Vec<Block>>;

pub type ImblHamt = Stack<ImblHashTable<ArcK>, ImblHashDir<ArcK>, VecBlocks>;
pub type ImblHamtTriomphe = Stack<ImblHashTable<ArcTK>, ImblHashDir<ArcTK>, VecBlocks>;
pub type ImblBTree = Stack<ImblOrdTable<ArcK>, ImblOrdDir<ArcK>, VecBlocks>;
pub type ImHamt = Stack<ImHashTable, ImHashDir, VecBlocks>;
pub type RpdsHamt = Stack<RpdsHashTable, RpdsHashDir, VecBlocks>;
pub type RpdsRbt = Stack<RpdsRbtTable, RpdsRbtDir, VecBlocks>;
pub type ChunkMap = Stack<ChunkTable, ChunkDir, VecBlocks>;
pub type HandRadix = Stack<RadixTable, BTreeDir, VecBlocks>;
pub type HandFlat = Stack<FlatTable, BTreeDir, VecBlocks>;

pub type ContentArcVec = Stack<ImblHashTable<ArcK>, ImblHashDir<ArcK>, ArcBytes>;
pub type ContentVecBlocks = ImblHamt;
pub type ContentImblVector = Stack<ImblHashTable<ArcK>, ImblHashDir<ArcK>, Blocks<imbl::GenericVector<Block, ArcK>>>;
pub type ContentRpdsVector = Stack<ImblHashTable<ArcK>, ImblHashDir<ArcK>, Blocks<rpds::VectorSync<Block>>>;
pub type ContentRadix = Stack<ImblHashTable<ArcK>, ImblHashDir<ArcK>, Blocks<RadixBlocks>>;

/// Tudo de crate: imbl `HashMap` na tabela e nos diretórios, `imbl::Vector` de blocos.
pub type FinalImbl = Stack<ImblHashTable<ArcK>, ImblHashDir<ArcK>, Blocks<imbl::GenericVector<Block, ArcK>>>;
/// Tabela à mão (trie de raiz 64), diretório imbl `HashMap`, blocos na sequência híbrida à mão.
pub type FinalHand = Stack<RadixTable, ImblHashDir<ArcK>, Blocks<RadixBlocks>>;
/// Como [`FinalHand`], mas diretório imbl `OrdMap` (readdir já sai em ordem de nome).
pub type FinalHandOrd = Stack<RadixTable, ImblOrdDir<ArcK>, Blocks<RadixBlocks>>;
/// Tudo de crate e ordenado: imbl `OrdMap` na tabela e nos diretórios, `imbl::Vector` de blocos.
pub type FinalImblOrd = Stack<ImblOrdTable<ArcK>, ImblOrdDir<ArcK>, Blocks<imbl::GenericVector<Block, ArcK>>>;

/// Metadados de um candidato pro JSON.
#[derive(Clone, Debug)]
pub struct Meta {
    pub key: &'static str,
    pub name: &'static str,
    /// Crates de terceiros envolvidas (nome no Cargo.lock), vazio se é tudo à mão.
    pub crates: &'static [&'static str],
    /// Tem parte feita à mão (código nosso pra manter).
    pub hand: bool,
}

pub const STRUCTURES: &[Meta] = &[
    Meta { key: "imbl-hamt", name: "imbl::HashMap (HAMT, std Arc)", crates: &["imbl"], hand: false },
    Meta {
        key: "imbl-hamt-triomphe",
        name: "imbl::HashMap (HAMT, triomphe::Arc)",
        crates: &["imbl", "triomphe"],
        hand: false,
    },
    Meta { key: "imbl-btree", name: "imbl::OrdMap (B-tree)", crates: &["imbl"], hand: false },
    Meta { key: "im-hamt", name: "im::HashMap 15 (HAMT, sem manutenção)", crates: &["im"], hand: false },
    Meta { key: "rpds-hamt", name: "rpds::HashTrieMapSync (HAMT)", crates: &["rpds"], hand: false },
    Meta { key: "rpds-rbtree", name: "rpds::RedBlackTreeMapSync", crates: &["rpds"], hand: false },
    Meta {
        key: "chunkmap",
        name: "immutable-chunkmap MapM (AVL de blocos)",
        crates: &["immutable-chunkmap"],
        hand: false,
    },
    Meta { key: "hand-radix", name: "à mão: trie de raiz 64 + Arc<BTreeMap> por diretório", crates: &[], hand: true },
    Meta {
        key: "hand-flat",
        name: "à mão: Arc<BTreeMap> da tabela inteira (controle negativo)",
        crates: &[],
        hand: true,
    },
];

pub const CONTENTS: &[Meta] = &[
    Meta { key: "content-arc-vec", name: "conteúdo Arc<Vec<u8>>", crates: &[], hand: true },
    Meta { key: "content-vec-blocks", name: "conteúdo Vec<Arc<bloco 4 KiB>>", crates: &[], hand: true },
    Meta { key: "content-imbl-vector", name: "conteúdo imbl::Vector<Arc<bloco 4 KiB>>", crates: &["imbl"], hand: false },
    Meta {
        key: "content-rpds-vector",
        name: "conteúdo rpds::VectorSync<Arc<bloco 4 KiB>>",
        crates: &["rpds"],
        hand: false,
    },
    Meta { key: "content-radix", name: "conteúdo à mão: Vec até 32 blocos + trie de raiz 64", crates: &[], hand: true },
];

pub const FINALS: &[Meta] = &[
    Meta {
        key: "final-imbl",
        name: "combinação imbl: HashMap + HashMap + Vector de blocos",
        crates: &["imbl"],
        hand: false,
    },
    Meta {
        key: "final-hand",
        name: "combinação: trie à mão + imbl::HashMap nos diretórios + blocos híbridos à mão",
        crates: &["imbl"],
        hand: true,
    },
    Meta {
        key: "final-hand-ord",
        name: "combinação: trie à mão + imbl::OrdMap nos diretórios + blocos híbridos à mão",
        crates: &["imbl"],
        hand: true,
    },
    Meta {
        key: "final-imbl-ord",
        name: "combinação imbl ordenada: OrdMap + OrdMap + Vector de blocos",
        crates: &["imbl"],
        hand: false,
    },
];

/// Chama `$body` uma vez por candidato de estrutura, com `$F` ligado ao tipo e `$meta` aos
/// metadados.
#[macro_export]
macro_rules! for_each_structure {
    (|$meta:ident, $F:ident| $body:block) => {{
        use $crate::flavors as fl;
        {
            type $F = fl::ImblHamt;
            let $meta = &fl::STRUCTURES[0];
            $body
        }
        {
            type $F = fl::ImblHamtTriomphe;
            let $meta = &fl::STRUCTURES[1];
            $body
        }
        {
            type $F = fl::ImblBTree;
            let $meta = &fl::STRUCTURES[2];
            $body
        }
        {
            type $F = fl::ImHamt;
            let $meta = &fl::STRUCTURES[3];
            $body
        }
        {
            type $F = fl::RpdsHamt;
            let $meta = &fl::STRUCTURES[4];
            $body
        }
        {
            type $F = fl::RpdsRbt;
            let $meta = &fl::STRUCTURES[5];
            $body
        }
        {
            type $F = fl::ChunkMap;
            let $meta = &fl::STRUCTURES[6];
            $body
        }
        {
            type $F = fl::HandRadix;
            let $meta = &fl::STRUCTURES[7];
            $body
        }
        {
            type $F = fl::HandFlat;
            let $meta = &fl::STRUCTURES[8];
            $body
        }
    }};
}

/// Como [`for_each_structure!`], pros candidatos de conteúdo.
#[macro_export]
macro_rules! for_each_content {
    (|$meta:ident, $F:ident| $body:block) => {{
        use $crate::flavors as fl;
        {
            type $F = fl::ContentArcVec;
            let $meta = &fl::CONTENTS[0];
            $body
        }
        {
            type $F = fl::ContentVecBlocks;
            let $meta = &fl::CONTENTS[1];
            $body
        }
        {
            type $F = fl::ContentImblVector;
            let $meta = &fl::CONTENTS[2];
            $body
        }
        {
            type $F = fl::ContentRpdsVector;
            let $meta = &fl::CONTENTS[3];
            $body
        }
        {
            type $F = fl::ContentRadix;
            let $meta = &fl::CONTENTS[4];
            $body
        }
    }};
}

/// Como [`for_each_structure!`], pras combinações finais.
#[macro_export]
macro_rules! for_each_final {
    (|$meta:ident, $F:ident| $body:block) => {{
        use $crate::flavors as fl;
        {
            type $F = fl::FinalImbl;
            let $meta = &fl::FINALS[0];
            $body
        }
        {
            type $F = fl::FinalHand;
            let $meta = &fl::FINALS[1];
            $body
        }
        {
            type $F = fl::FinalHandOrd;
            let $meta = &fl::FINALS[2];
            $body
        }
        {
            type $F = fl::FinalImblOrd;
            let $meta = &fl::FINALS[3];
            $body
        }
    }};
}
