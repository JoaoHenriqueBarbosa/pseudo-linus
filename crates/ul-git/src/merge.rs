//! A mesclagem de três vias de árvores, a estratégia `ort` do git: para cada caminho olha a base e
//! os dois lados, resolve o que um lado só mexeu, detecta renomeações de cada lado até a base (e as
//! junta com as mudanças do outro lado), faz a mesclagem de conteúdo linha a linha (`xmerge`) e
//! decide os conflitos (conteúdo, adição/adição, modificação/remoção, renomeação/remoção,
//! renomeação/renomeação, arquivo/diretório, tipos diferentes). O resultado é a árvore mesclada (com
//! os marcadores de conflito nos arquivos em conflito), os estágios de cada caminho em conflito e as
//! mensagens por caminho. Não faz a detecção de renomeação de diretório, nem submódulos.

use std::collections::{BTreeMap, BTreeSet};

use crate::diff::rename::{self, RenameOpts};
use crate::diff::{self, text};
use crate::error::R;
use crate::graph::Graph;
use crate::hash::{EMPTY_TREE, Kind, Oid};
use crate::index::{IEntry, Index};
use crate::object;
use crate::pathspec::Pathspec;
use crate::repo::Repo;
use crate::xmerge::{self, Favor, Style};

pub type Files = BTreeMap<Vec<u8>, (u32, Oid)>;

/// As opções da mesclagem: os nomes que aparecem nos marcadores e nas mensagens.
#[derive(Clone, Debug)]
pub struct Opts {
    /// Nosso lado (`HEAD`).
    pub branch1: String,
    /// O lado deles.
    pub branch2: String,
    /// A base, só mostrada no estilo `diff3`.
    pub ancestor: String,
    pub favor: Favor,
    pub style: Style,
    pub detect_renames: bool,
    /// Limiar de semelhança das renomeações (0 a 60000).
    pub rename_score: u32,
}

impl Opts {
    pub fn new(branch1: &str, branch2: &str, ancestor: &str) -> Opts {
        Opts {
            branch1: branch1.to_string(),
            branch2: branch2.to_string(),
            ancestor: ancestor.to_string(),
            favor: Favor::None,
            style: Style::Merge,
            detect_renames: true,
            rename_score: rename::DEFAULT_RENAME_SCORE,
        }
    }
}

/// Modo e id de uma versão do arquivo; modo 0 é "não existe".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vi {
    pub mode: u32,
    pub oid: Oid,
}

impl Vi {
    const NONE: Vi = Vi { mode: 0, oid: Oid::ZERO };

    fn of(e: Option<&(u32, Oid)>) -> Vi {
        match e {
            Some(&(mode, oid)) => Vi { mode, oid },
            None => Vi::NONE,
        }
    }
}

/// Um caminho em conflito: as versões da base e dos dois lados que vão pros estágios 1, 2 e 3 do
/// índice (o bit `i` de `filemask` diz se a versão `i` existe).
#[derive(Clone, Debug)]
pub struct Conflicted {
    pub path: Vec<u8>,
    pub stages: [Vi; 3],
    pub filemask: u8,
}

pub struct Outcome {
    pub clean: bool,
    /// A árvore mesclada, com as versões em conflito (marcadores, ou o lado que ficou).
    pub tree: Oid,
    pub conflicted: Vec<Conflicted>,
    /// As mensagens de cada caminho, na ordem em que o git as mostra.
    pub messages: BTreeMap<Vec<u8>, Vec<String>>,
}

impl Outcome {
    /// As mensagens no stdout, como o `merge_display_update_messages`.
    pub fn display(&self) {
        let mut out = String::new();
        for msgs in self.messages.values() {
            for m in msgs {
                out.push_str(m);
                out.push('\n');
            }
        }
        crate::os::outs(&out);
    }
}

/// O que se sabe de um caminho durante a mesclagem (o `conflict_info` do ort).
#[derive(Clone, Debug)]
struct Ci {
    stages: [Vi; 3],
    filemask: u8,
    dirmask: u8,
    match_mask: u8,
    pathnames: [Vec<u8>; 3],
    df_conflict: bool,
    path_conflict: bool,
    clean: bool,
    is_null: bool,
    result: Vi,
}

impl Ci {
    fn resolved(path: &[u8], stages: [Vi; 3], filemask: u8, dirmask: u8, result: Vi) -> Ci {
        Ci {
            stages,
            filemask,
            dirmask,
            match_mask: 0,
            pathnames: [path.to_vec(), path.to_vec(), path.to_vec()],
            df_conflict: false,
            path_conflict: false,
            clean: true,
            is_null: result.mode == 0,
            result,
        }
    }
}

struct RenamePair {
    src: Vec<u8>,
    dst: Vec<u8>,
    side: usize,
}

struct Engine<'a> {
    repo: &'a Repo,
    o: &'a Opts,
    /// Zero na mesclagem de verdade; acima disso é a mesclagem das bases (conflitos ficam com os
    /// marcadores e não saem mensagens).
    depth: u32,
    paths: BTreeMap<Vec<u8>, Ci>,
    msgs: BTreeMap<Vec<u8>, Vec<String>>,
}

fn lossy(p: &[u8]) -> String {
    String::from_utf8_lossy(p).into_owned()
}

fn branch_label(o: &Opts, side: usize) -> &str {
    if side == 1 { &o.branch1 } else { &o.branch2 }
}

/// `unique_path`: `caminho~ramo` (com `/` do ramo virando `_`), e `_N` se já existir.
fn unique_path(paths: &BTreeMap<Vec<u8>, Ci>, path: &[u8], branch: &str) -> Vec<u8> {
    let mut base = path.to_vec();
    base.push(b'~');
    base.extend(branch.bytes().map(|c| if c == b'/' { b'_' } else { c }));
    let mut cand = base.clone();
    let mut suffix = 0;
    while paths.contains_key(&cand) {
        cand = base.clone();
        cand.extend_from_slice(format!("_{suffix}").as_bytes());
        suffix += 1;
    }
    cand
}

impl Engine<'_> {
    fn path_msg(&mut self, path: &[u8], text: String) {
        if self.depth > 0 {
            return;
        }
        self.msgs.entry(path.to_vec()).or_default().push(text);
    }

    /// Reúne os caminhos: o que se resolve só olhando os três lados já nasce resolvido.
    fn collect(&mut self, base: &Files, s1: &Files, s2: &Files) {
        let mut dirs: [BTreeSet<Vec<u8>>; 3] = [BTreeSet::new(), BTreeSet::new(), BTreeSet::new()];
        for (i, f) in [base, s1, s2].iter().enumerate() {
            for p in f.keys() {
                let mut k = 0;
                while let Some(s) = p[k..].iter().position(|c| *c == b'/') {
                    dirs[i].insert(p[..k + s].to_vec());
                    k += s + 1;
                }
            }
        }
        let mut all: BTreeSet<&Vec<u8>> = BTreeSet::new();
        all.extend(base.keys());
        all.extend(s1.keys());
        all.extend(s2.keys());
        for path in all {
            let n = [base.get(path), s1.get(path), s2.get(path)];
            let stages = [Vi::of(n[0]), Vi::of(n[1]), Vi::of(n[2])];
            let mut filemask = 0u8;
            let mut dirmask = 0u8;
            for i in 0..3 {
                if n[i].is_some() {
                    filemask |= 1 << i;
                }
                if dirs[i].contains(path) {
                    dirmask |= 1 << i;
                }
            }
            let s1_matches_base = n[1].is_some() && n[0].is_some() && n[0] == n[1];
            let s2_matches_base = n[2].is_some() && n[0].is_some() && n[0] == n[2];
            let sides_match = n[1].is_some() && n[2].is_some() && n[1] == n[2];
            let mut match_mask = 0u8;
            if s1_matches_base {
                match_mask = if s2_matches_base { 7 } else { 3 };
            } else if s2_matches_base {
                match_mask = 5;
            } else if sides_match {
                match_mask = 6;
            }
            // Os três iguais: a base é o resultado.
            if s1_matches_base && s2_matches_base {
                self.paths.insert(path.clone(), Ci::resolved(path, stages, filemask, dirmask, stages[0]));
                continue;
            }
            if filemask == 7 && dirmask == 0 {
                let pick = if sides_match || s2_matches_base {
                    Some(stages[1])
                } else if s1_matches_base {
                    Some(stages[2])
                } else {
                    None
                };
                if let Some(r) = pick {
                    self.paths.insert(path.clone(), Ci::resolved(path, stages, filemask, dirmask, r));
                    continue;
                }
            }
            let mut ci = Ci::resolved(path, stages, filemask, dirmask, Vi::NONE);
            ci.clean = false;
            ci.df_conflict = filemask != 0 && dirmask != 0;
            ci.match_mask = match_mask;
            if dirmask != 0 {
                ci.match_mask &= filemask;
            }
            self.paths.insert(path.clone(), ci);
        }
    }

    /// As renomeações de cada lado até a base, pela detecção de renomeação do diff.
    fn detect_renames(&self, base: &Oid, s1: &Oid, s2: &Oid) -> R<Vec<RenamePair>> {
        let mut out: Vec<RenamePair> = Vec::new();
        if !self.o.detect_renames {
            return Ok(out);
        }
        for (side, tree) in [(1usize, s1), (2usize, s2)] {
            let pairs = diff::diff_trees(self.repo, Some(base), Some(tree), &Pathspec::default())?;
            let ro = RenameOpts { min_score: self.o.rename_score, ..RenameOpts::default() };
            for p in rename::detect(self.repo, pairs, &ro)? {
                if p.status == b'R' {
                    out.push(RenamePair { src: p.one.path.clone(), dst: p.two.path.clone(), side });
                }
            }
        }
        out.sort_by(|a, b| a.src.cmp(&b.src));
        Ok(out)
    }

    fn read_blob(&self, oid: &Oid) -> R<Vec<u8>> {
        if oid.is_zero() {
            return Ok(Vec::new());
        }
        let (_, d) = self.repo.read_object(oid)?;
        Ok(d.to_vec())
    }

    /// `merge_3way` + `ll_merge`: o texto mesclado e se sobrou conflito. Binário não se mescla.
    fn merge_3way(&mut self, path: &[u8], o: &Oid, a: &Oid, b: &Oid, pathnames: &[Vec<u8>; 3], extra_marker: usize) -> R<(Vec<u8>, bool)> {
        let (base_name, name1, name2) = if pathnames[0] == pathnames[1] && pathnames[1] == pathnames[2] {
            (self.o.ancestor.clone(), self.o.branch1.clone(), self.o.branch2.clone())
        } else {
            (
                format!("{}:{}", self.o.ancestor, lossy(&pathnames[0])),
                format!("{}:{}", self.o.branch1, lossy(&pathnames[1])),
                format!("{}:{}", self.o.branch2, lossy(&pathnames[2])),
            )
        };
        let orig = self.read_blob(o)?;
        let src1 = self.read_blob(a)?;
        let src2 = self.read_blob(b)?;
        if text::is_binary(&orig) || text::is_binary(&src1) || text::is_binary(&src2) {
            if self.depth > 0 {
                return Ok((orig, false));
            }
            return match self.o.favor {
                Favor::Ours => Ok((src1, false)),
                Favor::Theirs => Ok((src2, false)),
                _ => {
                    self.path_msg(path, format!("warning: Cannot merge binary files: {} ({} vs. {})", lossy(path), name1, name2));
                    Ok((src1, true))
                }
            };
        }
        let p = xmerge::Params {
            name1: &name1,
            name2: &name2,
            ancestor: &base_name,
            favor: if self.depth > 0 { Favor::None } else { self.o.favor },
            style: self.o.style,
            marker_size: 7 + extra_marker,
        };
        let m = xmerge::merge(&orig, &src1, &src2, &p);
        Ok((m.data, m.conflicts > 0))
    }

    /// `handle_content_merge`: modo e conteúdo de um arquivo mudado dos dois lados. Devolve se foi
    /// limpo e a versão resultante.
    fn handle_content_merge(&mut self, path: &[u8], o: Vi, a: Vi, b: Vi, pathnames: &[Vec<u8>; 3], extra_marker: usize) -> R<(bool, Vi)> {
        let mut clean = true;
        let mut result = Vi { mode: 0, oid: Oid::ZERO };
        if a.mode == b.mode || a.mode == o.mode {
            result.mode = b.mode;
        } else {
            result.mode = a.mode;
            clean = b.mode == o.mode;
        }
        if a.oid == b.oid || a.oid == o.oid {
            result.oid = b.oid;
        } else if b.oid == o.oid {
            result.oid = a.oid;
        } else if object::is_reg(a.mode) {
            let two_way = (o.mode & 0o170000) != (a.mode & 0o170000);
            let base_oid = if two_way { Oid::ZERO } else { o.oid };
            let (data, conflict) = self.merge_3way(path, &base_oid, &a.oid, &b.oid, pathnames, extra_marker)?;
            result.oid = self.repo.write_object(Kind::Blob, &data)?;
            if conflict {
                clean = false;
            }
            self.path_msg(path, format!("Auto-merging {}", lossy(path)));
        } else if object::is_gitlink(a.mode) {
            // Submódulos: sem história pra consultar, fica o nosso lado e o conflito.
            clean = false;
            result.oid = a.oid;
        } else if object::is_link(a.mode) {
            if self.depth > 0 {
                clean = false;
                result.mode = o.mode;
                result.oid = o.oid;
            } else {
                match self.o.favor {
                    Favor::Ours => result.oid = a.oid,
                    Favor::Theirs => result.oid = b.oid,
                    _ => {
                        clean = false;
                        result.oid = a.oid;
                    }
                }
            }
        }
        Ok((clean, result))
    }

    /// `process_renames`: junta os estágios do destino com os da origem e decide os conflitos de
    /// renomeação.
    fn process_renames(&mut self, pairs: &[RenamePair]) -> R<()> {
        let depth = self.depth as usize;
        let mut i = 0;
        while i < pairs.len() {
            let oldpath = pairs[i].src.clone();
            let newpath = pairs[i].dst.clone();
            let (Some(oldinfo), Some(newinfo)) = (self.paths.get(&oldpath).cloned(), self.paths.get(&newpath).cloned()) else {
                i += 1;
                continue;
            };
            if oldinfo.clean {
                i += 1;
                continue;
            }
            if i + 1 < pairs.len() && pairs[i + 1].src == oldpath {
                // O mesmo arquivo renomeado dos dois lados.
                let other = pairs[i + 1].dst.clone();
                let pathnames = [oldpath.clone(), newpath.clone(), other.clone()];
                let Some(mut base) = self.paths.get(&oldpath).cloned() else { break };
                let Some(mut side1) = self.paths.get(&newpath).cloned() else { break };
                let Some(mut side2) = self.paths.get(&other).cloned() else { break };
                if newpath == other {
                    side1.stages[0] = base.stages[0];
                    side1.filemask |= 1;
                    base.is_null = true;
                    base.clean = true;
                    self.paths.insert(newpath.clone(), side1);
                    self.paths.insert(oldpath.clone(), base);
                    i += 2;
                    continue;
                }
                let (clean_merge, mut merged) =
                    self.handle_content_merge(&oldpath, base.stages[0], side1.stages[1], side2.stages[2], &pathnames, 1 + 2 * depth)?;
                let was_binary = !clean_merge && merged == side1.stages[1];
                side1.stages[1] = merged;
                if was_binary {
                    merged = side2.stages[2];
                }
                side2.stages[2] = merged;
                side1.path_conflict = true;
                side2.path_conflict = true;
                base.path_conflict = true;
                let (b1, b2) = (self.o.branch1.clone(), self.o.branch2.clone());
                self.path_msg(
                    &pathnames[0],
                    format!(
                        "CONFLICT (rename/rename): {} renamed to {} in {} and to {} in {}.",
                        lossy(&pathnames[0]),
                        lossy(&pathnames[1]),
                        b1,
                        lossy(&pathnames[2]),
                        b2
                    ),
                );
                self.paths.insert(oldpath.clone(), base);
                self.paths.insert(newpath.clone(), side1);
                self.paths.insert(other.clone(), side2);
                i += 2;
                continue;
            }

            let mut oldinfo = oldinfo;
            let mut newinfo = newinfo;
            let target_index = pairs[i].side;
            let other_index = 3 - target_index;
            let old_sidemask = 1u8 << other_index;
            let source_deleted = oldinfo.filemask == 1;
            let mut collision = newinfo.filemask & old_sidemask != 0;
            let type_changed = !source_deleted && object::is_reg(oldinfo.stages[other_index].mode) != object::is_reg(newinfo.stages[target_index].mode);
            if type_changed && collision {
                collision = false;
            }
            let (rename_branch, delete_branch) = if target_index == 1 {
                (self.o.branch1.clone(), self.o.branch2.clone())
            } else {
                (self.o.branch2.clone(), self.o.branch1.clone())
            };
            if collision && !source_deleted {
                // Renomeação contra adição, ou duas renomeações pro mesmo destino.
                let mut pathnames = [oldpath.clone(), oldpath.clone(), oldpath.clone()];
                pathnames[other_index] = oldpath.clone();
                pathnames[target_index] = newpath.clone();
                let ent = |s: &Engine<'_>, name: &Vec<u8>| s.paths.get(name).cloned();
                let (Some(e0), Some(e1), Some(e2)) = (ent(self, &pathnames[0]), ent(self, &pathnames[1]), ent(self, &pathnames[2])) else {
                    i += 1;
                    continue;
                };
                let (clean, merged) =
                    self.handle_content_merge(&oldpath, e0.stages[0], e1.stages[1], e2.stages[2], &pathnames, 1 + 2 * depth)?;
                newinfo.stages[target_index] = merged;
                if !clean {
                    self.path_msg(
                        &newpath,
                        format!(
                            "CONFLICT (rename involved in collision): rename of {} -> {} has content conflicts AND collides with another path; this may result in nested conflict markers.",
                            lossy(&oldpath),
                            lossy(&newpath)
                        ),
                    );
                }
            } else if collision && source_deleted {
                newinfo.path_conflict = true;
                self.path_msg(
                    &newpath,
                    format!("CONFLICT (rename/delete): {} renamed to {} in {}, but deleted in {}.", lossy(&oldpath), lossy(&newpath), rename_branch, delete_branch),
                );
            } else {
                newinfo.stages[0] = oldinfo.stages[0];
                newinfo.filemask |= 1;
                newinfo.pathnames[0] = oldpath.clone();
                if type_changed {
                    oldinfo.stages[0] = Vi::NONE;
                    oldinfo.filemask &= 0x06;
                } else if source_deleted {
                    newinfo.path_conflict = true;
                    self.path_msg(
                        &newpath,
                        format!("CONFLICT (rename/delete): {} renamed to {} in {}, but deleted in {}.", lossy(&oldpath), lossy(&newpath), rename_branch, delete_branch),
                    );
                } else {
                    newinfo.stages[other_index] = oldinfo.stages[other_index];
                    newinfo.filemask |= 1 << other_index;
                    newinfo.pathnames[other_index] = oldpath.clone();
                }
            }
            if !type_changed {
                oldinfo.is_null = true;
                oldinfo.clean = true;
            }
            self.paths.insert(oldpath, oldinfo);
            self.paths.insert(newpath, newinfo);
            i += 1;
        }
        Ok(())
    }

    /// `process_entry`: decide um caminho ainda não resolvido. Devolve as entradas que ocupam o lugar
    /// dele (mais de uma quando os tipos diferem, ou o arquivo sai do caminho de um diretório).
    fn process_entry(&mut self, path: &[u8], mut ci: Ci, dir_remains: bool) -> R<Vec<(Vec<u8>, Ci)>> {
        let mut path: Vec<u8> = path.to_vec();
        let mut df_file_index = 0usize;
        if ci.df_conflict && !dir_remains {
            // O diretório não atrapalha mais: o arquivo ocupa o lugar.
            ci.df_conflict = false;
            ci.clean = false;
            ci.is_null = false;
            ci.match_mask &= !ci.dirmask;
            ci.dirmask = 0;
            for i in 0..3 {
                if ci.filemask & (1 << i) == 0 {
                    ci.stages[i] = Vi::NONE;
                }
            }
        } else if ci.df_conflict {
            // O diretório continua aqui, então o arquivo muda de lugar.
            if ci.filemask == 1 {
                return Ok(Vec::new());
            }
            let old_path = path.clone();
            ci.match_mask &= !ci.dirmask;
            let dirmask = ci.dirmask;
            ci.dirmask = 0;
            for i in 0..3 {
                if ci.filemask & (1 << i) == 0 {
                    ci.stages[i] = Vi::NONE;
                }
            }
            df_file_index = if dirmask & 2 != 0 { 2 } else { 1 };
            let branch = branch_label(self.o, df_file_index).to_string();
            path = unique_path(&self.paths, &path, &branch);
            self.path_msg(
                &path.clone(),
                format!(
                    "CONFLICT (file/directory): directory in the way of {} from {}; moving it to {} instead.",
                    lossy(&old_path),
                    branch,
                    lossy(&path)
                ),
            );
        }

        let fmt = |m: u32| m & 0o170000;
        if ci.match_mask != 0 {
            ci.clean = !ci.df_conflict && !ci.path_conflict;
            if ci.match_mask == 6 {
                ci.result = ci.stages[1];
            } else {
                let othermask = 7 & !ci.match_mask;
                let side = if othermask == 4 { 2 } else { 1 };
                ci.result = ci.stages[side];
                ci.is_null = ci.result.mode == 0;
                if ci.is_null {
                    ci.clean = true;
                }
            }
        } else if ci.filemask >= 6 && fmt(ci.stages[1].mode) != fmt(ci.stages[2].mode) {
            // Dois tipos diferentes (arquivo, link, submódulo).
            if self.depth > 0 {
                ci.clean = false;
                ci.result = ci.stages[0];
                ci.is_null = ci.result.mode == 0;
            } else {
                let (o_mode, a_mode, b_mode) = (ci.stages[0].mode, ci.stages[1].mode, ci.stages[2].mode);
                let (rename_a, rename_b) = if object::is_reg(a_mode) {
                    (true, false)
                } else if object::is_reg(b_mode) {
                    (false, true)
                } else {
                    (true, true)
                };
                let (b1, b2) = (self.o.branch1.clone(), self.o.branch2.clone());
                let a_path = if rename_a { Some(unique_path(&self.paths, &path, &b1)) } else { None };
                let b_path = if rename_b { Some(unique_path(&self.paths, &path, &b2)) } else { None };
                let text = if rename_a && rename_b {
                    format!(
                        "CONFLICT (distinct types): {} had different types on each side; renamed both of them so each can be recorded somewhere.",
                        lossy(&path)
                    )
                } else {
                    format!(
                        "CONFLICT (distinct types): {} had different types on each side; renamed one of them so each can be recorded somewhere.",
                        lossy(&path)
                    )
                };
                self.path_msg(&path.clone(), text);
                ci.clean = false;
                let mut new_ci = ci.clone();
                new_ci.result = ci.stages[2];
                new_ci.stages[1] = Vi::NONE;
                new_ci.filemask = 5;
                if fmt(b_mode) != fmt(o_mode) {
                    new_ci.stages[0] = Vi::NONE;
                    new_ci.filemask = 4;
                }
                ci.result = ci.stages[1];
                ci.stages[2] = Vi::NONE;
                ci.filemask = 3;
                if fmt(a_mode) != fmt(o_mode) {
                    ci.stages[0] = Vi::NONE;
                    ci.filemask = 2;
                }
                let a_key = a_path.unwrap_or_else(|| path.clone());
                let b_key = b_path.unwrap_or_else(|| path.clone());
                return Ok(vec![(a_key, ci), (b_key, new_ci)]);
            }
        } else if ci.filemask >= 6 {
            let (o, a, b) = (ci.stages[0], ci.stages[1], ci.stages[2]);
            let pathnames = ci.pathnames.clone();
            let (clean_merge, merged_file) = self.handle_content_merge(&path, o, a, b, &pathnames, self.depth as usize * 2)?;
            ci.clean = clean_merge && !ci.df_conflict && !ci.path_conflict;
            ci.result = merged_file;
            ci.is_null = merged_file.mode == 0;
            if clean_merge && ci.df_conflict && df_file_index != 0 {
                ci.filemask = 1 << df_file_index;
                ci.stages[df_file_index] = merged_file;
            }
            if !clean_merge {
                let reason = if object::is_gitlink(merged_file.mode) {
                    "submodule"
                } else if ci.filemask == 6 {
                    "add/add"
                } else {
                    "content"
                };
                self.path_msg(&path.clone(), format!("CONFLICT ({reason}): Merge conflict in {}", lossy(&path)));
            }
        } else if ci.filemask == 3 || ci.filemask == 5 {
            // Modificado de um lado e removido do outro.
            let side = if ci.filemask == 5 { 2 } else { 1 };
            let index = if self.depth > 0 { 0 } else { side };
            ci.result = ci.stages[index];
            ci.clean = false;
            let (modify_branch, delete_branch) = if side == 1 {
                (self.o.branch1.clone(), self.o.branch2.clone())
            } else {
                (self.o.branch2.clone(), self.o.branch1.clone())
            };
            if ci.path_conflict && ci.stages[0].oid == ci.stages[side].oid {
                // Veio de uma renomeação/remoção: o conteúdo não mudou, então não há o que avisar.
            } else {
                self.path_msg(
                    &path.clone(),
                    format!(
                        "CONFLICT (modify/delete): {} deleted in {} and modified in {}.  Version {} of {} left in tree.",
                        lossy(&path),
                        delete_branch,
                        modify_branch,
                        modify_branch,
                        lossy(&path)
                    ),
                );
            }
        } else if ci.filemask == 2 || ci.filemask == 4 {
            let side = if ci.filemask == 4 { 2 } else { 1 };
            ci.result = ci.stages[side];
            ci.clean = !ci.df_conflict && !ci.path_conflict;
        } else if ci.filemask == 1 {
            ci.is_null = true;
            ci.result = Vi::NONE;
            ci.clean = !ci.path_conflict;
        }
        Ok(vec![(path, ci)])
    }

    /// `process_entries`: o que não nasceu resolvido, primeiro os caminhos sem arquivo/diretório
    /// em disputa e depois os outros, que precisam saber se o diretório sobrou.
    fn process_entries(&mut self) -> R<()> {
        for df_pass in [false, true] {
            let keys: Vec<Vec<u8>> = self.paths.iter().filter(|(_, ci)| !ci.clean && ci.df_conflict == df_pass).map(|(k, _)| k.clone()).collect();
            for key in keys {
                let Some(ci) = self.paths.remove(&key) else { continue };
                let mut prefix = key.clone();
                prefix.push(b'/');
                let dir_remains = df_pass
                    && self
                        .paths
                        .range(prefix.clone()..)
                        .take_while(|(k, _)| k.starts_with(&prefix))
                        .any(|(_, c)| !c.is_null && c.result.mode != 0);
                for (k, c) in self.process_entry(&key, ci, dir_remains)? {
                    self.paths.insert(k, c);
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> R<Outcome> {
        let mut entries: Vec<(&[u8], u32, Oid)> = Vec::new();
        let mut conflicted: Vec<Conflicted> = Vec::new();
        for (path, ci) in &self.paths {
            if !ci.is_null && ci.result.mode != 0 {
                entries.push((path.as_slice(), ci.result.mode, ci.result.oid));
            }
            if !ci.clean {
                conflicted.push(Conflicted { path: path.clone(), stages: ci.stages, filemask: ci.filemask });
            }
        }
        let tree = self.repo.write_tree_entries(&entries)?;
        Ok(Outcome { clean: conflicted.is_empty(), tree, conflicted, messages: self.msgs })
    }
}

/// Mescla três árvores (a base e os dois lados) com as opções dadas.
pub fn merge_trees(repo: &Repo, o: &Opts, depth: u32, base: &Oid, side1: &Oid, side2: &Oid) -> R<Outcome> {
    let fb: Files = repo.flatten_tree(base)?;
    let f1: Files = repo.flatten_tree(side1)?;
    let f2: Files = repo.flatten_tree(side2)?;
    let mut eng = Engine { repo, o, depth, paths: BTreeMap::new(), msgs: BTreeMap::new() };
    eng.collect(&fb, &f1, &f2);
    let pairs = eng.detect_renames(base, side1, side2)?;
    eng.process_renames(&pairs)?;
    eng.process_entries()?;
    eng.finish()
}

/// A mesclagem recursiva de dois commits: várias bases se mesclam antes numa base virtual (os
/// conflitos dessa mesclagem ficam com os marcadores). `bases` vem do mais antigo pro mais novo.
pub fn merge_commits(repo: &Repo, o: &Opts, h1: &Oid, h2: &Oid, bases: &[Oid]) -> R<Outcome> {
    let mut o = o.clone();
    let (base_tree, ancestor) = match bases {
        [] => (EMPTY_TREE, "empty tree".to_string()),
        [one] => (repo.tree_of(one)?, repo.abbrev_default(one)),
        [first, rest @ ..] => {
            let mut graph = Graph::new(repo);
            let mut cur_tree = repo.tree_of(first)?;
            for next in rest {
                let inner = graph.merge_bases(first, &[*next])?;
                let (inner_base, inner_name) = match inner.first() {
                    Some(b) if inner.len() == 1 => (repo.tree_of(b)?, repo.abbrev_default(b)),
                    Some(b) => (repo.tree_of(b)?, "merged common ancestors".to_string()),
                    None => (EMPTY_TREE, "empty tree".to_string()),
                };
                let mut io = Opts::new("Temporary merge branch 1", "Temporary merge branch 2", &inner_name);
                io.style = o.style;
                io.detect_renames = o.detect_renames;
                io.rename_score = o.rename_score;
                let next_tree = repo.tree_of(next)?;
                let r = merge_trees(repo, &io, 1, &inner_base, &cur_tree, &next_tree)?;
                cur_tree = r.tree;
            }
            (cur_tree, "merged common ancestors".to_string())
        }
    };
    o.ancestor = ancestor;
    let t1 = repo.tree_of(h1)?;
    let t2 = repo.tree_of(h2)?;
    merge_trees(repo, &o, 0, &base_tree, &t1, &t2)
}

/// Leva o índice e a árvore de trabalho da árvore do HEAD pro resultado (mantendo o que é local) e
/// registra os caminhos em conflito nos estágios 1, 2 e 3. Se algo local seria sobrescrito, nada é
/// tocado e o erro do git sai (`Fail::Exit(1)`).
pub fn checkout_result(repo: &Repo, head_tree: &Oid, out: &Outcome) -> R<()> {
    use crate::cmd::unpack::{self, Opts as UnpackOpts};
    let ipath = repo.index_path();
    let idx = Index::load(&ipath)?;
    let uo = UnpackOpts { verb: "merge", advice: "merge", force: false };
    let mut new_idx = unpack::switch_tree(repo, &idx, Some(head_tree), &out.tree, &uo)?;
    for c in &out.conflicted {
        new_idx.remove(&c.path);
        for i in 0..3 {
            if c.filemask & (1 << i) == 0 {
                continue;
            }
            let v = c.stages[i];
            let mut e = IEntry::bare(c.path.clone(), v.oid, v.mode);
            e.stage = (i + 1) as u8;
            new_idx.insert_raw(e);
        }
    }
    new_idx.write(&ipath)
}

/// Os nomes (separados por espaço) que o índice tem diferentes do HEAD: o texto do `error` que o
/// git dá quando o índice difere do HEAD antes de mesclar. Vazio se o índice bate com o HEAD.
pub fn unclean_names(repo: &Repo, head_tree: &Oid) -> R<Vec<u8>> {
    let idx = Index::load(&repo.index_path())?;
    let pairs = diff::diff_tree_index(repo, Some(head_tree), &idx, &Pathspec::default())?;
    let names: Vec<Vec<u8>> = pairs.iter().map(|p| p.path().to_vec()).collect();
    Ok(names.join(&b' '))
}
