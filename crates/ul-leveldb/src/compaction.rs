//! A escolha das compactações (`VersionSet::PickCompaction`, `SetupOtherInputs`, `AddBoundaryInputs` e a classe
//! `Compaction` de `db/version_set.cc`): qual nível compactar, quais tabelas entram dele e do nível seguinte, e as
//! regras que decidem onde uma tabela de saída termina e se uma chave encoberta pode ser descartada. A execução (o
//! merge e a escrita das tabelas) fica no `db.rs`.

use std::cmp::Ordering;

use crate::comparator;
use crate::dbformat::*;
use crate::version::{total_file_size, Version};
use crate::version_edit::{FileMeta, VersionEdit};

#[derive(Debug)]
pub struct Compaction {
    /// O nível de origem; as tabelas de saída vão para `level + 1`.
    pub level: usize,
    /// As tabelas do nível de origem e as do nível seguinte que elas sobrepõem.
    pub inputs: [Vec<FileMeta>; 2],
    /// A mudança de versão que a compactação acumula (o ponteiro de compactação e, depois, as tabelas).
    pub edit: VersionEdit,
    max_file_size: usize,
    /// As tabelas do nível `level + 2` que a faixa sobrepõe: limitam o quanto uma saída pode sobrepor lá.
    grandparents: Vec<FileMeta>,
    grandparent_index: usize,
    seen_key: bool,
    overlapped_bytes: u64,
    /// As tabelas dos níveis abaixo do de saída, para saber se uma remoção pode ser descartada (`IsBaseLevelForKey`).
    deeper: Vec<FileMeta>,
}

/// `FindLargestKey` e `FindSmallestKey` sobre um conjunto de tabelas.
fn get_range(files: &[FileMeta]) -> (Vec<u8>, Vec<u8>) {
    let mut smallest = files[0].smallest.clone();
    let mut largest = files[0].largest.clone();
    for file in &files[1..] {
        if compare_internal(&file.smallest, &smallest) == Ordering::Less {
            smallest = file.smallest.clone();
        }
        if compare_internal(&file.largest, &largest) == Ordering::Greater {
            largest = file.largest.clone();
        }
    }
    (smallest, largest)
}

fn get_range2(first: &[FileMeta], second: &[FileMeta]) -> (Vec<u8>, Vec<u8>) {
    let all: Vec<FileMeta> = first.iter().chain(second).cloned().collect();
    get_range(&all)
}

/// `AddBoundaryInputs`: uma tabela do nível que começa na mesma chave de usuário em que uma das escolhidas termina
/// (as versões de uma chave se espalham por duas tabelas) entra junto, senão a versão mais velha ficaria para trás.
fn add_boundary_inputs(level_files: &[FileMeta], chosen: &mut Vec<FileMeta>) {
    while !chosen.is_empty() {
        let (_, largest) = get_range(chosen);
        let boundary = level_files
            .iter()
            .filter(|file| compare_internal(&file.smallest, &largest) == Ordering::Greater && comparator::compare(user_key(&file.smallest), user_key(&largest)) == Ordering::Equal)
            .min_by(|a, b| compare_internal(&a.smallest, &b.smallest));
        match boundary {
            Some(file) => chosen.push(file.clone()),
            None => break,
        }
    }
}

/// `VersionSet::PickCompaction` para a compactação por tamanho: o nível de maior nota, a primeira tabela depois do
/// ponteiro de compactação e o que `SetupOtherInputs` junta a ela. `None` quando nenhum nível passa de nota 1.
/// (A compactação por seeks não existe aqui: ela nasce de leituras que erram a primeira tabela, o que o Local Storage
/// quase não faz.)
pub fn pick(version: &Version, pointers: &[Vec<u8>; NUM_LEVELS], max_file_size: usize, info: &mut dyn FnMut(&str)) -> Option<Compaction> {
    if version.compaction_score < 1.0 {
        return None;
    }
    let level = version.compaction_level as usize;
    let files = &version.files[level];
    let first = files.iter().find(|file| pointers[level].is_empty() || compare_internal(&file.largest, &pointers[level]) == Ordering::Greater).or_else(|| files.first())?;
    let mut inputs0 = vec![first.clone()];
    if level == 0 {
        // As tabelas do nível 0 se sobrepõem: entram todas as que tocam a faixa (que a busca alarga).
        let (smallest, largest) = get_range(&inputs0);
        inputs0 = version.overlapping_inputs(0, user_key(&smallest), user_key(&largest));
    }
    let mut compaction = Compaction {
        level,
        inputs: [inputs0, Vec::new()],
        edit: VersionEdit::default(),
        max_file_size,
        grandparents: Vec::new(),
        grandparent_index: 0,
        seen_key: false,
        overlapped_bytes: 0,
        deeper: version.files[(level + 2).min(NUM_LEVELS)..].iter().flatten().cloned().collect(),
    };
    compaction.setup_other_inputs(version, info);
    Some(compaction)
}

/// `VersionSet::CompactRange`: as tabelas do nível que cobrem `[begin, end]` (chaves do usuário) e o que
/// `SetupOtherInputs` junta a elas. Acima do nível 0 a entrada para quando passa do tamanho de um arquivo, para a
/// compactação manual andar em pedaços. `None` quando nada no nível cobre a faixa.
pub fn manual(version: &Version, level: usize, begin: &[u8], end: &[u8], max_file_size: usize, info: &mut dyn FnMut(&str)) -> Option<Compaction> {
    let mut inputs0 = version.overlapping_inputs(level, begin, end);
    if inputs0.is_empty() {
        return None;
    }
    if level > 0 {
        let mut total = 0;
        if let Some(stop) = inputs0.iter().position(|file| {
            total += file.file_size;
            total >= max_file_size as u64
        }) {
            inputs0.truncate(stop + 1);
        }
    }
    let mut compaction = Compaction {
        level,
        inputs: [inputs0, Vec::new()],
        edit: VersionEdit::default(),
        max_file_size,
        grandparents: Vec::new(),
        grandparent_index: 0,
        seen_key: false,
        overlapped_bytes: 0,
        deeper: version.files[(level + 2).min(NUM_LEVELS)..].iter().flatten().cloned().collect(),
    };
    compaction.setup_other_inputs(version, info);
    Some(compaction)
}

impl Compaction {
    /// `VersionSet::SetupOtherInputs`.
    fn setup_other_inputs(&mut self, version: &Version, info: &mut dyn FnMut(&str)) {
        let level = self.level;
        add_boundary_inputs(&version.files[level], &mut self.inputs[0]);
        let (smallest, mut largest) = get_range(&self.inputs[0]);
        self.inputs[1] = version.overlapping_inputs(level + 1, user_key(&smallest), user_key(&largest));
        add_boundary_inputs(&version.files[level + 1], &mut self.inputs[1]);
        let (mut all_start, mut all_limit) = get_range2(&self.inputs[0], &self.inputs[1]);
        // Dá para trazer mais tabelas do nível de origem sem trazer nenhuma a mais do seguinte? Então traz.
        if !self.inputs[1].is_empty() {
            let mut expanded0 = version.overlapping_inputs(level, user_key(&all_start), user_key(&all_limit));
            add_boundary_inputs(&version.files[level], &mut expanded0);
            let inputs1_size = total_file_size(&self.inputs[1]);
            let expanded0_size = total_file_size(&expanded0);
            if expanded0.len() > self.inputs[0].len() && inputs1_size + expanded0_size < 25 * self.max_file_size as u64 {
                let (new_start, new_limit) = get_range(&expanded0);
                let mut expanded1 = version.overlapping_inputs(level + 1, user_key(&new_start), user_key(&new_limit));
                add_boundary_inputs(&version.files[level + 1], &mut expanded1);
                if expanded1.len() == self.inputs[1].len() {
                    info(&format!(
                        "Expanding@{level} {}+{} ({}+{} bytes) to {}+{} ({}+{} bytes)",
                        self.inputs[0].len(),
                        self.inputs[1].len(),
                        total_file_size(&self.inputs[0]),
                        inputs1_size,
                        expanded0.len(),
                        expanded1.len(),
                        expanded0_size,
                        inputs1_size
                    ));
                    largest = new_limit;
                    self.inputs = [expanded0, expanded1];
                    (all_start, all_limit) = get_range2(&self.inputs[0], &self.inputs[1]);
                }
            }
        }
        if level + 2 < NUM_LEVELS {
            self.grandparents = version.overlapping_inputs(level + 2, user_key(&all_start), user_key(&all_limit));
        }
        self.edit.compact_pointers.push((level as u32, largest));
    }

    /// Uma tabela só, sem par no nível seguinte e sem muito para sobrepor depois: basta mudá-la de nível.
    pub fn is_trivial_move(&self) -> bool {
        self.inputs[0].len() == 1 && self.inputs[1].is_empty() && total_file_size(&self.grandparents) <= 10 * self.max_file_size as u64
    }

    /// `Compaction::ShouldStopBefore`: a tabela de saída em andamento já sobrepõe tanto do nível `level + 2` que é hora
    /// de começar outra antes desta chave.
    pub fn should_stop_before(&mut self, internal_key: &[u8]) -> bool {
        while self.grandparent_index < self.grandparents.len() && compare_internal(internal_key, &self.grandparents[self.grandparent_index].largest) == Ordering::Greater {
            if self.seen_key {
                self.overlapped_bytes += self.grandparents[self.grandparent_index].file_size;
            }
            self.grandparent_index += 1;
        }
        self.seen_key = true;
        if self.overlapped_bytes > 10 * self.max_file_size as u64 {
            self.overlapped_bytes = 0;
            return true;
        }
        false
    }

    /// `Compaction::IsBaseLevelForKey`: nenhum nível abaixo do de saída tem a chave, então uma remoção dela pode sumir.
    pub fn is_base_level_for_key(&self, user_key_bytes: &[u8]) -> bool {
        !self.deeper.iter().any(|file| comparator::compare(user_key(&file.smallest), user_key_bytes) != Ordering::Greater && comparator::compare(user_key_bytes, user_key(&file.largest)) != Ordering::Greater)
    }

    /// `Compaction::AddInputDeletions`.
    pub fn add_input_deletions(&mut self) {
        for (which, files) in self.inputs.iter().enumerate() {
            for file in files {
                self.edit.deleted_files.push(((self.level + which) as u32, file.number));
            }
        }
    }

    /// O tamanho máximo de uma tabela de saída (`MaxOutputFileSize`).
    pub fn max_output_file_size(&self) -> usize {
        self.max_file_size
    }
}
