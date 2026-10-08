//! `fts3_tokenize_vtab.c`: o módulo virtual `fts3tokenize`, que expõe um tokenizador do FTS3 como
//! tabela. Uma tabela é criada assim:
//!
//! ```text
//!   CREATE VIRTUAL TABLE <tbl> USING fts3tokenize(<nome-do-tokenizador>, <arg-1>, ...);
//! ```
//!
//! e tem o esquema `CREATE TABLE <tbl>(input, token, start, end, position)`. A consulta precisa
//! ter `input = <string>` no WHERE; o módulo tokeniza a string e devolve uma linha por token:
//!
//! * `input`: uma cópia da string;
//! * `token`: um token da entrada;
//! * `start`: o deslocamento do token na entrada, em bytes;
//! * `end`: o deslocamento do byte logo depois do fim do token;
//! * `position`: o índice do token na entrada.
//!
//! Sem argumentos o tokenizador é `simple`. O módulo não tem representação persistente, então
//! `xCreate` e `xConnect` são a mesma operação (e por isso a tabela epônima existe).
//!
//! Modelo v2: o `Fts3tokTable` guarda só o tokenizador (`pMod` some: o `Rc<dyn Fts3Tokenizer>` já
//! é o módulo instanciado). O cursor copia a entrada e o token corrente (o C guarda ponteiros).

use std::any::Any;
use std::rc::Rc;

use crate::build::text_arg;
use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{SQLITE_DONE, SQLITE_ERROR, SQLITE_INDEX_CONSTRAINT_EQ, SQLITE_OK};
use crate::mem::{Mem, StrDtor};
use crate::printf::mprintf;
use crate::vdbeapi::{result_int, result_text, text_of};
use crate::vtab::{create_module, declare_vtab};

use super::int::{fts3_dequote, Fts3Tokenizer, Fts3TokenizerCursor};
use super::tokenizer::Fts3HashWrapper;

/// `FTS3_TOK_SCHEMA`: o esquema da tabela do tokenizador.
const FTS3_TOK_SCHEMA: &[u8] = b"CREATE TABLE x(input, token, start, end, position)";

/// `Fts3tokTable`.
struct Fts3tokTable {
    /// `base.zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
    /// `pMod` e `pTok`: o tokenizador.
    p_tok: Rc<dyn Fts3Tokenizer>,
}

/// `Fts3tokCursor`.
#[derive(Default)]
struct Fts3tokCursor {
    /// `zInput`: a entrada (cópia).
    z_input: Option<Vec<u8>>,
    /// `pCsr`: o cursor do tokenizador sobre `z_input`.
    p_csr: Option<Box<dyn Fts3TokenizerCursor>>,
    /// `iRowid`.
    i_rowid: i32,
    /// `zToken`/`nToken`: o valor corrente de `token` (`None` é o fim dos resultados).
    z_token: Option<Vec<u8>>,
    /// `iStart`.
    i_start: i32,
    /// `iEnd`.
    i_end: i32,
    /// `iPos`.
    i_pos: i32,
}

/// A tabela `Fts3tokTable` de uma instância `Vtab`.
fn tok_table(vtab: &mut dyn Vtab) -> &mut Fts3tokTable {
    vtab.as_any_mut().downcast_mut::<Fts3tokTable>().expect("fts3tokenize: instância de outro módulo")
}

/// `fts3tokDequoteArray` sobre os `argv` do `xCreate`/`xConnect` depois do nome da tabela: uma
/// cópia de cada um, sem as aspas.
fn fts3tok_dequote_array(argv: &[Vec<u8>]) -> Vec<Vec<u8>> {
    argv.iter()
        .map(|a| {
            let n = a.iter().position(|&c| c == 0).unwrap_or(a.len());
            let mut z = a[..n].to_vec();
            fts3_dequote(&mut z);
            z
        })
        .collect()
}

/// `fts3tokConnectMethod`: o trabalho de `xConnect` e de `xCreate`.
///
/// ```text
///   argv[0]: nome do módulo
///   argv[1]: nome do banco
///   argv[2]: nome da tabela
///   argv[3]: primeiro argumento (nome do tokenizador)
/// ```
fn fts3tok_connect(
    db: &mut Connection,
    p_hash: &Option<Rc<dyn Any>>,
    argv: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Box<dyn Vtab>, i32> {
    let rc = declare_vtab(db, FTS3_TOK_SCHEMA);
    if rc != SQLITE_OK {
        return Err(rc);
    }

    let az_dequote = fts3tok_dequote_array(argv.get(3..).unwrap_or(&[]));
    let z_module: &[u8] = az_dequote.first().map_or(b"simple".as_slice(), |z| z.as_slice());

    /* `fts3tokQueryTokenizer`: procura o tokenizador pelo nome no registro. */
    let p_module = p_hash
        .as_ref()
        .and_then(|a| Rc::clone(a).downcast::<Fts3HashWrapper>().ok())
        .and_then(|h| h.find_module(z_module));
    let Some(p_mod) = p_module else {
        *pz_err = mprintf(b"unknown tokenizer: %s", &[text_arg(z_module)]);
        return Err(SQLITE_ERROR);
    };

    let az_arg: &[Vec<u8>] = if az_dequote.len() > 1 { &az_dequote[1..] } else { &[] };
    let p_tok = p_mod.create(az_arg)?;
    Ok(Box::new(Fts3tokTable { z_err_msg: None, p_tok }))
}

impl Vtab for Fts3tokTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `fts3tokBestIndexMethod`: usa só um `input = ?` utilizável.
    fn best_index(&mut self, _db: &mut Connection, p_info: &mut IndexInfo) -> i32 {
        if p_info.a_constraint_usage.len() < p_info.a_constraint.len() {
            p_info.a_constraint_usage.resize(p_info.a_constraint.len(), Default::default());
        }
        for (i, c) in p_info.a_constraint.iter().enumerate() {
            if c.usable && c.i_column == 0 && c.op as i32 == SQLITE_INDEX_CONSTRAINT_EQ {
                p_info.idx_num = 1;
                p_info.a_constraint_usage[i].argv_index = 1;
                p_info.a_constraint_usage[i].omit = true;
                p_info.estimated_cost = 1.0;
                return SQLITE_OK;
            }
        }

        p_info.idx_num = 0;
        debug_assert!(p_info.estimated_cost > 1000000.0);

        SQLITE_OK
    }

    /// `fts3tokDisconnectMethod`: soltar a tabela solta o tokenizador (`xDestroy`).
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `xDestroy` é o mesmo `fts3tokDisconnectMethod`.
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `fts3tokOpenMethod`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(Fts3tokCursor::default()))
    }
}

impl Fts3tokCursor {
    /// `fts3tokResetCursor`: devolve o cursor ao estado de recém-aberto. Soltar `p_csr` é o
    /// `xClose` do tokenizador.
    fn reset(&mut self) {
        self.p_csr = None;
        self.z_input = None;
        self.z_token = None;
        self.i_start = 0;
        self.i_end = 0;
        self.i_pos = 0;
        self.i_rowid = 0;
    }

    /// O trabalho de `fts3tokNextMethod`.
    fn advance(&mut self) -> i32 {
        self.i_rowid += 1;
        let rc = match self.p_csr.as_mut() {
            Some(csr) => match csr.next() {
                Ok(t) => {
                    self.z_token = Some(t.z.to_vec());
                    self.i_start = t.i_start_offset;
                    self.i_end = t.i_end_offset;
                    self.i_pos = t.i_position;
                    SQLITE_OK
                }
                Err(rc) => rc,
            },
            None => SQLITE_ERROR,
        };

        if rc != SQLITE_OK {
            self.reset();
            if rc == SQLITE_DONE {
                return SQLITE_OK;
            }
        }
        rc
    }
}

impl VtabCursor for Fts3tokCursor {
    /// `fts3tokCloseMethod`.
    fn close(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.reset();
        SQLITE_OK
    }

    /// `fts3tokFilterMethod`.
    fn filter(
        &mut self,
        _db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        _idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let mut rc = SQLITE_ERROR;

        self.reset();
        if idx_num == 1 {
            let z_input: Vec<u8> = argv
                .first()
                .and_then(|a| text_of(a))
                .map(|z| z.into_owned())
                .unwrap_or_default();
            match tok_table(vtab).p_tok.open(&z_input) {
                Ok(csr) => {
                    self.p_csr = Some(csr);
                    rc = SQLITE_OK;
                }
                Err(e) => rc = e,
            }
            self.z_input = Some(z_input);
        }

        if rc != SQLITE_OK {
            return rc;
        }
        self.advance()
    }

    /// `fts3tokNextMethod`.
    fn next(&mut self, _db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.advance()
    }

    /// `fts3tokEofMethod`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        self.z_token.is_none() as i32
    }

    /// `fts3tokColumnMethod`: `CREATE TABLE x(input, token, start, end, position)`.
    fn column(&mut self, _vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i_col: i32) -> i32 {
        match i_col {
            0 => match self.z_input.as_deref() {
                /* `sqlite3_result_text(..., -1, ...)`: o texto vale até o primeiro NUL. */
                Some(z) => {
                    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
                    result_text(ctx, Some(&z[..n]), n as i32, StrDtor::Transient)
                }
                None => result_text(ctx, None, -1, StrDtor::Transient),
            },
            1 => match self.z_token.as_deref() {
                Some(z) => result_text(ctx, Some(z), z.len() as i32, StrDtor::Transient),
                None => result_text(ctx, None, 0, StrDtor::Transient),
            },
            2 => result_int(ctx, self.i_start),
            3 => result_int(ctx, self.i_end),
            _ => {
                debug_assert!(i_col == 4);
                result_int(ctx, self.i_pos)
            }
        }
        SQLITE_OK
    }

    /// `fts3tokRowidMethod`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, p_rowid: &mut i64) -> i32 {
        *p_rowid = self.i_rowid as i64;
        SQLITE_OK
    }
}

/// `fts3tok_module`: o `xCreate` e o `xConnect` são a mesma função, então o módulo tem tabela
/// epônima.
struct Fts3tokModule;

impl VtabModule for Fts3tokModule {
    fn i_version(&self) -> i32 {
        0
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps { create: true, ..ModuleCaps::default() }
    }

    fn create_is_connect(&self) -> bool {
        true
    }

    /// `fts3tokConnectMethod` como `xCreate`.
    fn x_create(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts3tok_connect(db, aux, argv, err)
    }

    /// `fts3tokConnectMethod`.
    fn x_connect(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts3tok_connect(db, aux, argv, err)
    }
}

/// `sqlite3Fts3InitTok`: registra o módulo `fts3tokenize` na conexão. `p_hash` é o registro de
/// tokenizadores (o `pAux`); o `xDestroy` (`hashDestroy`) é o `Drop` do `Rc`.
pub fn fts3_init_tok(db: &mut Connection, p_hash: Rc<Fts3HashWrapper>) -> i32 {
    create_module(db, b"fts3tokenize", Some(Rc::new(Fts3tokModule)), Some(p_hash), None)
}
