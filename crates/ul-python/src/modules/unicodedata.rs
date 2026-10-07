//! `unicodedata`: as propriedades de caractere do Unicode 15.1.0 (a versão do CPython 3.13) e o
//! objeto `ucd_3_2_0` com as do 3.2.0, os dois lidos dos arquivos oficiais do UCD (`ucd.rs`).

use std::rc::Rc;

use crate::modules::ucd::{self, Props};

type Db = dyn Props;
use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn one_char(fname: &str, v: Option<&Value>) -> PyResult<u32> {
    match v {
        Some(Value::Str(s)) => {
            let mut it = s.as_str().chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Ok(u32::from(c)),
                _ => Err(type_error(format!("{fname}() argument must be a unicode character, not str"))),
            }
        }
        Some(other) => Err(type_error(format!("{fname}() argument must be a unicode character, not {}", other.type_name()))),
        None => Err(type_error(format!("{fname}() takes at least 1 argument (0 given)"))),
    }
}

/// Categoria geral de `c` no banco atual (`str.istitle` e afins).
pub(crate) fn category_code(c: char) -> &'static str {
    ucd::current().category(u32::from(c))
}

/// Troca os dígitos decimais Unicode (`Nd`) de um texto não ASCII pelos ASCII (`int('٣')` é 3).
/// `None` se o texto já é ASCII ou não tem nada a trocar.
pub fn fold_decimal_digits(s: &str) -> Option<String> {
    if s.is_ascii() {
        return None;
    }
    let db = ucd::current();
    let mut changed = false;
    let out: String = s
        .chars()
        .map(|c| match (c.is_ascii(), db.decimal(u32::from(c))) {
            (false, Some(d)) => {
                changed = true;
                char::from_digit(d as u32, 10).unwrap_or(c)
            }
            _ => c,
        })
        .collect();
    changed.then_some(out)
}

/// Uma função do módulo, aplicada ao banco `db`.
fn call(db: &'static Db, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    match name {
        "category" => {
            let a = bind("category", args, kw, &["chr"], 1)?;
            Ok(Value::str(db.category(one_char("category", a[0].as_ref())?)))
        }
        "bidirectional" => {
            let a = bind("bidirectional", args, kw, &["chr"], 1)?;
            Ok(Value::str(db.bidirectional(one_char("bidirectional", a[0].as_ref())?)))
        }
        "east_asian_width" => {
            let a = bind("east_asian_width", args, kw, &["chr"], 1)?;
            Ok(Value::str(db.east_asian_width(one_char("east_asian_width", a[0].as_ref())?)))
        }
        "combining" => {
            let a = bind("combining", args, kw, &["chr"], 1)?;
            Ok(Value::Int(i64::from(db.combining(one_char("combining", a[0].as_ref())?))))
        }
        "mirrored" => {
            let a = bind("mirrored", args, kw, &["chr"], 1)?;
            Ok(Value::Int(i64::from(db.mirrored(one_char("mirrored", a[0].as_ref())?))))
        }
        "decomposition" => {
            let a = bind("decomposition", args, kw, &["chr"], 1)?;
            Ok(Value::str(db.decomposition(one_char("decomposition", a[0].as_ref())?)))
        }
        "decimal" | "digit" | "numeric" => {
            let fname: &'static str = match name {
                "decimal" => "decimal",
                "digit" => "digit",
                _ => "numeric",
            };
            let a = bind(fname, args, kw, &["chr", "default"], 1)?;
            let cp = one_char(fname, a[0].as_ref())?;
            let value = match fname {
                "decimal" => db.decimal(cp).map(Value::Int),
                "digit" => db.digit(cp).map(Value::Int),
                _ => db.numeric(cp).map(Value::Float),
            };
            match (value, a[1].clone()) {
                (Some(v), _) => Ok(v),
                (None, Some(d)) => Ok(d),
                (None, None) => Err(exc(
                    "ValueError",
                    match fname {
                        "decimal" => "not a decimal",
                        "digit" => "not a digit",
                        _ => "not a numeric character",
                    },
                )),
            }
        }
        "name" => {
            let a = bind("name", args, kw, &["chr", "default"], 1)?;
            let cp = one_char("name", a[0].as_ref())?;
            match (db.name(cp), a[1].clone()) {
                (Some(n), _) => Ok(Value::str(n)),
                (None, Some(d)) => Ok(d),
                (None, None) => Err(exc("ValueError", "no such name")),
            }
        }
        "lookup" => {
            let a = bind("lookup", args, kw, &["name"], 1)?;
            let text = match a[0].as_ref() {
                Some(Value::Str(s)) => s.as_str().to_string(),
                Some(Value::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
                Some(other) => return Err(type_error(format!("lookup() argument must be str, not {}", other.type_name()))),
                None => return Err(type_error("lookup() takes exactly one argument (0 given)")),
            };
            if text.len() > 256 {
                return Err(exc("KeyError", "name too long"));
            }
            match db.lookup(&text) {
                Some(cps) => Ok(Value::str(cps.into_iter().filter_map(char::from_u32).collect::<String>())),
                None => Err(exc("KeyError", format!("undefined character name '{text}'"))),
            }
        }
        "normalize" | "is_normalized" => {
            let fname: &'static str = if name == "normalize" { "normalize" } else { "is_normalized" };
            let a = bind(fname, args, kw, &["form", "unistr"], 2)?;
            let form = match a[0].as_ref() {
                Some(Value::Str(f)) => f.as_str().to_string(),
                Some(other) => return Err(type_error(format!("{fname}() argument 1 must be str, not {}", other.type_name()))),
                None => return Err(type_error(format!("{fname}() takes exactly 2 arguments"))),
            };
            let text = match a[1].as_ref() {
                Some(Value::Str(s)) => s.as_str().to_string(),
                Some(other) => return Err(type_error(format!("{fname}() argument 2 must be str, not {}", other.type_name()))),
                None => return Err(type_error(format!("{fname}() takes exactly 2 arguments"))),
            };
            let Some(out) = db.normalize(&form, &text) else {
                return Err(exc("ValueError", "invalid normalization form"));
            };
            Ok(if fname == "normalize" { Value::str(out) } else { Value::Bool(out == text) })
        }
        other => Err(exc("AttributeError", format!("'unicodedata.UCD' object has no attribute '{other}'"))),
    }
}

const FUNCTIONS: &[&str] = &[
    "bidirectional",
    "category",
    "combining",
    "decimal",
    "decomposition",
    "digit",
    "east_asian_width",
    "is_normalized",
    "lookup",
    "mirrored",
    "name",
    "normalize",
    "numeric",
];

/// `unicodedata.UCD`: o tipo de `ucd_3_2_0`, com as mesmas funções do módulo sobre outro banco.
struct Ucd {
    db: &'static Db,
}

impl ExtObject for Ucd {
    fn type_name(&self) -> &'static str {
        "unicodedata.UCD"
    }

    fn methods(&self) -> &'static [&'static str] {
        FUNCTIONS
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        (name == "unidata_version").then(|| Ok(Value::str(self.db.version())))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        call(self.db, name, args, kw)
    }
}

macro_rules! module_fn {
    ($($f:ident),*) => {
        $(fn $f(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
            call(ucd::current(), stringify!($f), args, kw)
        })*
    };
}

module_fn!(
    bidirectional,
    category,
    combining,
    decimal,
    decomposition,
    digit,
    east_asian_width,
    is_normalized,
    lookup,
    mirrored,
    name,
    normalize,
    numeric
);

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("unicodedata")
        .func("category", category)
        .func("combining", combining)
        .func("east_asian_width", east_asian_width)
        .func("bidirectional", bidirectional)
        .func("mirrored", mirrored)
        .func("decomposition", decomposition)
        .func("name", name)
        .func("lookup", lookup)
        .func("normalize", normalize)
        .func("is_normalized", is_normalized)
        .func("decimal", decimal)
        .func("digit", digit)
        .func("numeric", numeric)
        .value("unidata_version", Value::str(ucd::current().version))
        .value("ucd_3_2_0", Value::Ext(Rc::new(Ucd { db: ucd::v3_2() })))
        .build()
}
