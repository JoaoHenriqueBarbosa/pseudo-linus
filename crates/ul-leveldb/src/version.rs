//! As versões do banco e o MANIFEST (`db/version_set.cc`, `db/version_edit.cc`): quais tabelas existem em cada nível,
//! o contador de números de arquivo, a última sequência e o log vigente. O MANIFEST é um log de `VersionEdit`; ao
//! abrir, o leveldb do Chromium REAPROVEITA o MANIFEST existente (`reuse_logs`) em vez de escrever um novo.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::comparator;
use crate::dbformat::*;
use crate::env::Env;
use crate::log::{read_records, Writer};
use crate::version_edit::{FileMeta, VersionEdit};
use crate::{Error, Result};

fn corruption(message: impl Into<String>) -> Error {
    Error::Corruption(message.into())
}

/// As tabelas de cada nível, ordenadas pela menor chave (e pelo número, no empate).
#[derive(Debug, Clone, Default)]
pub struct Version {
    pub files: [Vec<FileMeta>; NUM_LEVELS],
    pub compaction_score: f64,
    pub compaction_level: i32,
}

fn max_bytes_for_level(level: usize) -> f64 {
    let mut result = 10.0 * 1_048_576.0;
    let mut level = level;
    while level > 1 {
        result *= 10.0;
        level -= 1;
    }
    result
}

pub fn total_file_size(files: &[FileMeta]) -> u64 {
    files.iter().map(|file| file.file_size).sum()
}

impl Version {
    pub fn num_files(&self, level: usize) -> usize {
        self.files[level].len()
    }

    /// `VersionSet::LevelSummary`: `files[ 0 1 0 0 0 0 0 ]`, quantas tabelas há em cada nível.
    pub fn level_summary(&self) -> String {
        let counts: String = self.files.iter().map(|files| format!(" {}", files.len())).collect();
        format!("files[{counts} ]")
    }

    /// `VersionSet::Finalize`: o nível que mais precisa de compactação e a nota dele.
    fn finalize(&mut self) {
        let mut best_level = -1;
        let mut best_score = -1.0;
        for level in 0..NUM_LEVELS - 1 {
            let score = if level == 0 {
                self.files[0].len() as f64 / L0_COMPACTION_TRIGGER as f64
            } else {
                total_file_size(&self.files[level]) as f64 / max_bytes_for_level(level)
            };
            if score > best_score {
                best_level = level as i32;
                best_score = score;
            }
        }
        self.compaction_level = best_level;
        self.compaction_score = best_score;
    }

    /// Alguma tabela do nível cobre parte de `[smallest, largest]` (chaves do usuário).
    pub fn overlap_in_level(&self, level: usize, smallest: &[u8], largest: &[u8]) -> bool {
        self.files[level].iter().any(|file| !(comparator::compare(user_key(&file.largest), smallest) == Ordering::Less || comparator::compare(user_key(&file.smallest), largest) == Ordering::Greater))
    }

    /// `Version::GetOverlappingInputs`: as tabelas do nível que cobrem parte de `[begin, end]` (chaves do usuário). No
    /// nível 0 as tabelas se sobrepõem entre si, então uma tabela que alarga a faixa reinicia a busca.
    pub fn overlapping_inputs(&self, level: usize, begin: &[u8], end: &[u8]) -> Vec<FileMeta> {
        let (mut begin, mut end) = (begin.to_vec(), end.to_vec());
        let mut inputs: Vec<FileMeta> = Vec::new();
        let mut index = 0;
        while index < self.files[level].len() {
            let file = &self.files[level][index];
            index += 1;
            let (file_start, file_limit) = (user_key(&file.smallest), user_key(&file.largest));
            if comparator::compare(file_limit, &begin) == Ordering::Less || comparator::compare(file_start, &end) == Ordering::Greater {
                continue;
            }
            inputs.push(file.clone());
            if level == 0 {
                if comparator::compare(file_start, &begin) == Ordering::Less {
                    begin = file_start.to_vec();
                } else if comparator::compare(file_limit, &end) == Ordering::Greater {
                    end = file_limit.to_vec();
                } else {
                    continue;
                }
                inputs.clear();
                index = 0;
            }
        }
        inputs
    }

    /// `Version::PickLevelForMemTableOutput`: o nível em que a memtable vira tabela.
    pub fn pick_level_for_memtable_output(&self, smallest: &[u8], largest: &[u8], max_file_size: usize) -> usize {
        let mut level = 0;
        if !self.overlap_in_level(0, smallest, largest) {
            while level < MAX_MEM_COMPACT_LEVEL {
                if self.overlap_in_level(level + 1, smallest, largest) {
                    break;
                }
                if level + 2 < NUM_LEVELS {
                    let overlaps = self.overlapping_inputs(level + 2, smallest, largest);
                    if total_file_size(&overlaps) > 10 * max_file_size as u64 {
                        break;
                    }
                }
                level += 1;
            }
        }
        level
    }
}

fn by_smallest_key(a: &FileMeta, b: &FileMeta) -> Ordering {
    compare_internal(&a.smallest, &b.smallest).then(a.number.cmp(&b.number))
}

/// O conjunto de versões e o que o MANIFEST persiste.
#[derive(Debug)]
pub struct VersionSet {
    dir: String,
    max_file_size: usize,
    reuse_logs: bool,
    current: Version,
    next_file_number: u64,
    manifest_file_number: u64,
    last_sequence: u64,
    log_number: u64,
    prev_log_number: u64,
    compact_pointers: [Vec<u8>; NUM_LEVELS],
    descriptor: Option<(String, Writer)>,
}

impl VersionSet {
    pub fn new(dir: &str, max_file_size: usize, reuse_logs: bool) -> VersionSet {
        VersionSet {
            dir: dir.to_owned(),
            max_file_size,
            reuse_logs,
            current: Version::default(),
            next_file_number: 2,
            manifest_file_number: 0,
            last_sequence: 0,
            log_number: 0,
            prev_log_number: 0,
            compact_pointers: Default::default(),
            descriptor: None,
        }
    }

    pub fn current(&self) -> &Version {
        &self.current
    }

    pub fn compact_pointers(&self) -> &[Vec<u8>; NUM_LEVELS] {
        &self.compact_pointers
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    pub fn set_last_sequence(&mut self, sequence: u64) {
        self.last_sequence = sequence;
    }

    pub fn log_number(&self) -> u64 {
        self.log_number
    }

    pub fn prev_log_number(&self) -> u64 {
        self.prev_log_number
    }

    pub fn manifest_file_number(&self) -> u64 {
        self.manifest_file_number
    }

    pub fn new_file_number(&mut self) -> u64 {
        let number = self.next_file_number;
        self.next_file_number += 1;
        number
    }

    pub fn mark_file_number_used(&mut self, number: u64) {
        if self.next_file_number <= number {
            self.next_file_number = number + 1;
        }
    }

    /// Os números das tabelas vivas.
    pub fn live_files(&self) -> BTreeSet<u64> {
        self.current.files.iter().flatten().map(|file| file.number).collect()
    }

    /// `VersionSet::Recover`: lê o `CURRENT` e o MANIFEST. Devolve se um MANIFEST novo precisa ser gravado (não, quando
    /// o existente é reaproveitado).
    pub fn recover(&mut self, env: &dyn Env, info_log: &mut dyn FnMut(&str)) -> Result<bool> {
        let current = env.read_file(&current_file_name(&self.dir))?;
        if current.last() != Some(&b'\n') {
            return Err(corruption("CURRENT file does not end with newline"));
        }
        let manifest_base = String::from_utf8_lossy(&current[..current.len() - 1]).into_owned();
        let manifest_path = format!("{}/{}", self.dir, manifest_base);
        let contents = env.read_file(&manifest_path).map_err(|error| match error {
            Error::Io(message) => Error::Io(format!("{}: CURRENT points to a non-existent file ({message})", self.dir)),
            other => other,
        })?;
        let mut base = Version::default();
        let (mut next_file, mut log_number, mut prev_log_number, mut last_sequence) = (None, None, None, None);
        for record in read_records(&contents)? {
            let edit = VersionEdit::decode(&record)?;
            if let Some(name) = &edit.comparator {
                if name != comparator::name() {
                    return Err(corruption(format!("{} does not match existing comparator : {}", String::from_utf8_lossy(name), String::from_utf8_lossy(comparator::name()))));
                }
            }
            apply_edit(&mut base, &mut self.compact_pointers, &edit);
            log_number = edit.log_number.or(log_number);
            prev_log_number = edit.prev_log_number.or(prev_log_number);
            next_file = edit.next_file_number.or(next_file);
            last_sequence = edit.last_sequence.or(last_sequence);
        }
        let next_file = next_file.ok_or_else(|| corruption("no meta-nextfile entry in descriptor"))?;
        let log_number = log_number.ok_or_else(|| corruption("no meta-lognumber entry in descriptor"))?;
        let last_sequence = last_sequence.ok_or_else(|| corruption("no last-sequence-number entry in descriptor"))?;
        let prev_log_number = prev_log_number.unwrap_or(0);
        self.mark_file_number_used(prev_log_number);
        self.mark_file_number_used(log_number);
        for files in &mut base.files {
            files.sort_by(by_smallest_key);
        }
        base.finalize();
        self.current = base;
        self.manifest_file_number = next_file;
        self.next_file_number = next_file + 1;
        self.last_sequence = last_sequence;
        self.log_number = log_number;
        self.prev_log_number = prev_log_number;
        Ok(!self.reuse_manifest(env, &manifest_path, &manifest_base, contents.len() as u64, info_log))
    }

    /// `VersionSet::ReuseManifest`: continua gravando no MANIFEST que já existe, se ele ainda é pequeno.
    fn reuse_manifest(&mut self, _env: &dyn Env, path: &str, base: &str, size: u64, info_log: &mut dyn FnMut(&str)) -> bool {
        if !self.reuse_logs {
            return false;
        }
        let Some((number, FileType::Descriptor)) = parse_file_name(base) else { return false };
        if size >= self.max_file_size as u64 {
            return false;
        }
        info_log(&format!("Reusing MANIFEST {path}"));
        self.descriptor = Some((path.to_owned(), Writer::resuming(size)));
        self.manifest_file_number = number;
        true
    }

    /// `VersionSet::LogAndApply`: grava a mudança no MANIFEST e a aplica à versão corrente.
    pub fn log_and_apply(&mut self, env: &dyn Env, edit: &mut VersionEdit) -> Result<()> {
        if edit.log_number.is_none() {
            edit.log_number = Some(self.log_number);
        }
        if edit.prev_log_number.is_none() {
            edit.prev_log_number = Some(self.prev_log_number);
        }
        edit.next_file_number = Some(self.next_file_number);
        edit.last_sequence = Some(self.last_sequence);
        let mut next = self.current.clone();
        let mut pointers = self.compact_pointers.clone();
        apply_edit(&mut next, &mut pointers, edit);
        for files in &mut next.files {
            files.sort_by(by_smallest_key);
        }
        next.finalize();
        let mut new_manifest: Option<String> = None;
        if self.descriptor.is_none() {
            let path = descriptor_file_name(&self.dir, self.manifest_file_number);
            edit.next_file_number = Some(self.next_file_number);
            let mut writer = Writer::new();
            let snapshot = self.snapshot_edit();
            env.write_file(&path, &writer.add_record(&snapshot.encode()))?;
            self.descriptor = Some((path.clone(), writer));
            new_manifest = Some(path);
        }
        let record = edit.encode();
        let result = match &mut self.descriptor {
            Some((path, writer)) => env.append_file(path, &writer.add_record(&record)),
            None => Ok(()),
        };
        if let Err(error) = result {
            if let Some(path) = new_manifest {
                self.descriptor = None;
                let _ = env.remove_file(&path);
            }
            return Err(error);
        }
        if new_manifest.is_some() {
            set_current_file(env, &self.dir, self.manifest_file_number)?;
        }
        self.current = next;
        self.compact_pointers = pointers;
        self.log_number = edit.log_number.unwrap_or(self.log_number);
        self.prev_log_number = edit.prev_log_number.unwrap_or(self.prev_log_number);
        Ok(())
    }

    /// `VersionSet::WriteSnapshot`: o estado inteiro, como o primeiro registro de um MANIFEST novo.
    fn snapshot_edit(&self) -> VersionEdit {
        let mut edit = VersionEdit { comparator: Some(comparator::name().to_vec()), ..VersionEdit::default() };
        for (level, pointer) in self.compact_pointers.iter().enumerate() {
            if !pointer.is_empty() {
                edit.compact_pointers.push((level as u32, pointer.clone()));
            }
        }
        for (level, files) in self.current.files.iter().enumerate() {
            for file in files {
                edit.new_files.push((level as u32, file.clone()));
            }
        }
        edit
    }
}

/// O que a edição muda na versão: tabelas apagadas, tabelas novas e os ponteiros de compactação.
fn apply_edit(version: &mut Version, pointers: &mut [Vec<u8>; NUM_LEVELS], edit: &VersionEdit) {
    for (level, key) in &edit.compact_pointers {
        if let Some(slot) = pointers.get_mut(*level as usize) {
            *slot = key.clone();
        }
    }
    for (level, number) in &edit.deleted_files {
        if let Some(files) = version.files.get_mut(*level as usize) {
            files.retain(|file| file.number != *number);
        }
    }
    for (level, file) in &edit.new_files {
        if let Some(files) = version.files.get_mut(*level as usize) {
            files.retain(|existing| existing.number != file.number);
            files.push(file.clone());
        }
    }
}

/// `SetCurrentFile`: grava `MANIFEST-<n>` num temporário e o renomeia para `CURRENT`.
pub fn set_current_file(env: &dyn Env, dir: &str, descriptor_number: u64) -> Result<()> {
    let temp = temp_file_name(dir, descriptor_number);
    env.write_file(&temp, format!("MANIFEST-{descriptor_number:06}\n").as_bytes())?;
    if let Err(error) = env.rename(&temp, &current_file_name(dir)) {
        let _ = env.remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::MemEnv;

    fn file(number: u64, smallest: &str, largest: &str) -> FileMeta {
        FileMeta {
            number,
            file_size: 100,
            smallest: internal_key(smallest.as_bytes(), 1, ValueType::Value),
            largest: internal_key(largest.as_bytes(), 1, ValueType::Value),
        }
    }

    #[test]
    fn a_new_database_manifest_is_recovered_and_reused() {
        let env = MemEnv::new();
        let mut record = VersionEdit { comparator: Some(comparator::name().to_vec()), log_number: Some(0), next_file_number: Some(2), last_sequence: Some(0), ..VersionEdit::default() };
        let manifest = Writer::new().add_record(&record.encode());
        record.log_number = Some(0);
        env.write_file("/db/MANIFEST-000001", &manifest).unwrap();
        set_current_file(&env, "/db", 1).unwrap();
        let mut versions = VersionSet::new("/db", 2 << 20, true);
        let mut messages = Vec::new();
        let save = versions.recover(&env, &mut |message| messages.push(message.to_owned())).unwrap();
        assert!(!save);
        assert_eq!(messages, vec!["Reusing MANIFEST /db/MANIFEST-000001".to_owned()]);
        // Depois de reaproveitar o MANIFEST 1, o primeiro número novo é 3 (o log do banco novo).
        assert_eq!(versions.new_file_number(), 3);
        assert_eq!(versions.manifest_file_number(), 1);
    }

    #[test]
    fn log_and_apply_appends_to_the_reused_manifest_and_recovers_the_files() {
        let env = MemEnv::new();
        let edit = VersionEdit { comparator: Some(comparator::name().to_vec()), log_number: Some(0), next_file_number: Some(2), last_sequence: Some(0), ..VersionEdit::default() };
        env.write_file("/db/MANIFEST-000001", &Writer::new().add_record(&edit.encode())).unwrap();
        set_current_file(&env, "/db", 1).unwrap();
        let mut versions = VersionSet::new("/db", 2 << 20, true);
        versions.recover(&env, &mut |_| {}).unwrap();
        let number = versions.new_file_number();
        let mut add = VersionEdit { log_number: Some(4), prev_log_number: Some(0), ..VersionEdit::default() };
        add.new_files.push((0, file(number, "a", "m")));
        versions.set_last_sequence(7);
        versions.log_and_apply(&env, &mut add).unwrap();
        assert_eq!(versions.current().num_files(0), 1);

        let mut again = VersionSet::new("/db", 2 << 20, true);
        again.recover(&env, &mut |_| {}).unwrap();
        assert_eq!(again.current().num_files(0), 1);
        assert_eq!((again.log_number(), again.last_sequence()), (4, 7));
    }

    #[test]
    fn a_memtable_output_with_no_overlap_goes_to_level_two() {
        let mut version = Version::default();
        assert_eq!(version.pick_level_for_memtable_output(b"a", b"b", 2 << 20), 2);
        version.files[0].push(file(5, "a", "c"));
        assert_eq!(version.pick_level_for_memtable_output(b"b", b"b", 2 << 20), 0);
        version.files[0].clear();
        version.files[1].push(file(6, "a", "c"));
        assert_eq!(version.pick_level_for_memtable_output(b"b", b"b", 2 << 20), 0);
        assert_eq!(version.pick_level_for_memtable_output(b"x", b"y", 2 << 20), 2);
    }
}
