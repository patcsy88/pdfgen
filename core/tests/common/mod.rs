//! Arlington PDF Model checker (ISO 32000-2, PDF 2.0) for the PDFs this crate writes.
//!
//! Loads the vendored TSV models in `tests/arlington`, walks every object reachable
//! from the trailer and checks each key against its model row: type, version,
//! deprecation, required, direct/indirect, possible values, special-case predicates
//! and keys the model does not define. A predicate the checker cannot evaluate, or a
//! link to a model that is not vendored, is reported as an error, never skipped.
#![allow(dead_code)]

pub mod content;

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{HashMap, HashSet};

const PDF_VERSION: f64 = 2.0;
const STD14: [&str; 14] = [
    "Times-Roman", "Times-Bold", "Times-Italic", "Times-BoldItalic", "Helvetica", "Helvetica-Bold",
    "Helvetica-Oblique", "Helvetica-BoldOblique", "Courier", "Courier-Bold", "Courier-Oblique",
    "Courier-BoldOblique", "Symbol", "ZapfDingbats",
];

struct Row {
    key: String,
    types: String,
    since: String,
    deprecated: String,
    required: String,
    indirect: String,
    inheritable: bool,
    default: String,
    possible: String,
    special: String,
    link: String,
}

struct Model {
    name: String,
    rows: Vec<Row>,
}

impl Model {
    fn row(&self, key: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.key == key).or_else(|| self.rows.iter().find(|r| r.key == "*"))
    }
}

fn load_models() -> HashMap<String, Model> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/arlington");
    let mut out = HashMap::new();
    for e in std::fs::read_dir(&dir).expect("vendored Arlington TSV directory") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "tsv") {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&p).unwrap();
            let rows = text
                .lines()
                .skip(1)
                .map(|l| {
                    let c: Vec<&str> = l.split('\t').collect();
                    let g = |i: usize| c.get(i).copied().unwrap_or("").to_string();
                    Row {
                        key: g(0),
                        types: g(1),
                        since: g(2),
                        deprecated: g(3),
                        required: g(4),
                        indirect: g(5),
                        inheritable: g(6) == "TRUE",
                        default: g(7),
                        possible: g(8),
                        special: g(9),
                        link: g(10),
                    }
                })
                .collect();
            out.insert(name.clone(), Model { name, rows });
        }
    }
    out
}

/// Splits on `sep` outside (), [].
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut start, mut depth) = (Vec::new(), 0, 0i32);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn strip_brackets(s: &str) -> &str {
    s.strip_prefix('[').and_then(|x| x.strip_suffix(']')).unwrap_or(s)
}

/// The cell content for type index `i` of `n` types (`[a];[b]` form or shared).
fn cell(s: &str, i: usize, n: usize) -> &str {
    if s.is_empty() {
        return s;
    }
    if n > 1 && s.starts_with('[') {
        return split_top(s, ';').get(i).map(|x| strip_brackets(x)).unwrap_or("");
    }
    strip_brackets(s)
}

/// Unwraps version predicates around a type, value or link name; `None` when the
/// item does not apply to PDF 2.0 without extensions.
fn unwrap_ver(item: &str) -> Option<&str> {
    let item = item.trim();
    let Some(rest) = item.strip_prefix("fn:") else { return Some(item) };
    let (name, args) = rest.split_once('(')?;
    let args = split_top(args.strip_suffix(')')?, ',');
    let v: f64 = args.first()?.trim().parse().ok().unwrap_or(f64::NAN);
    match (name, args.len()) {
        ("SinceVersion", 2) if v <= PDF_VERSION => unwrap_ver(args[1]),
        ("IsPDFVersion", 2) if v == PDF_VERSION => unwrap_ver(args[1]),
        ("BeforeVersion", 2) if PDF_VERSION < v => unwrap_ver(args[1]),
        ("Deprecated", 2) if PDF_VERSION < v => unwrap_ver(args[1]),
        _ => None,
    }
}

/// "SinceVersion" column: `None` when the row exists only under an extension.
fn since_version(s: &str) -> Option<f64> {
    if let Ok(v) = s.parse() {
        return Some(v);
    }
    let inner = s.strip_prefix("fn:Eval(")?.strip_suffix(')')?;
    inner.split(" || ").find_map(|p| p.trim().parse().ok())
}

#[derive(Debug)]
enum E {
    Fn(String, Vec<E>),
    Atom(String),
    Bin(String, Box<E>, Box<E>),
}

fn tokenize(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b' ' {
            i += 1;
            continue;
        }
        if let Some(t @ ("&&" | "||" | "==" | "!=" | "<=" | ">=")) = s.get(i..i + 2) {
            out.push(t);
            i += 2;
            continue;
        }
        // A spaced " - " is subtraction; otherwise '-' belongs to a number.
        let minus = b[i] == b'-' && i > 0 && b[i - 1] == b' ' && b.get(i + 1) == Some(&b' ');
        if minus || b"(),<>+*".contains(&b[i]) {
            out.push(&s[i..i + 1]);
            i += 1;
            continue;
        }
        let st = i;
        while i < b.len() && !b" (),<>=!&|+*".contains(&b[i]) {
            i += 1;
        }
        if i == st {
            i += 1; // stray operator character: kept so the parser rejects it
        }
        out.push(&s[st..i]);
    }
    out
}

struct Parser<'t> {
    t: Vec<&'t str>,
    i: usize,
}

impl<'t> Parser<'t> {
    fn peek(&self) -> Option<&'t str> {
        self.t.get(self.i).copied()
    }
    fn bump(&mut self) -> Option<&'t str> {
        self.i += 1;
        self.t.get(self.i - 1).copied()
    }
    fn binary(&mut self, ops: &[&str], next: fn(&mut Self) -> Result<E, String>) -> Result<E, String> {
        let mut l = next(self)?;
        while let Some(op) = self.peek().filter(|p| ops.contains(p)) {
            self.i += 1;
            l = E::Bin(op.to_string(), Box::new(l), Box::new(next(self)?));
        }
        Ok(l)
    }
    fn or(&mut self) -> Result<E, String> {
        self.binary(&["||"], Self::and)
    }
    fn and(&mut self) -> Result<E, String> {
        self.binary(&["&&"], Self::cmp)
    }
    fn cmp(&mut self) -> Result<E, String> {
        self.binary(&["==", "!=", "<", "<=", ">", ">="], Self::add)
    }
    fn add(&mut self) -> Result<E, String> {
        self.binary(&["+", "-"], Self::mul)
    }
    fn mul(&mut self) -> Result<E, String> {
        self.binary(&["*", "mod"], Self::prim)
    }
    fn prim(&mut self) -> Result<E, String> {
        match self.bump() {
            Some("(") => {
                let e = self.or()?;
                (self.bump() == Some(")")).then_some(e).ok_or_else(|| "expected `)`".into())
            }
            Some(f) if f.starts_with("fn:") => {
                if self.bump() != Some("(") {
                    return Err(format!("expected `(` after {f}"));
                }
                let mut args = Vec::new();
                if self.peek() == Some(")") {
                    self.i += 1;
                } else {
                    loop {
                        args.push(self.or()?);
                        match self.bump() {
                            Some(",") => continue,
                            Some(")") => break,
                            t => return Err(format!("unexpected {t:?} in {f} arguments")),
                        }
                    }
                }
                Ok(E::Fn(f.to_string(), args))
            }
            Some(a) if !"()<>=!&|+*,".contains(a) => Ok(E::Atom(a.to_string())),
            t => Err(format!("unexpected token {t:?}")),
        }
    }
}

fn parse(s: &str) -> Result<E, String> {
    let mut p = Parser { t: tokenize(s), i: 0 };
    let e = p.or().map_err(|m| format!("cannot parse `{s}`: {m}"))?;
    if p.i != p.t.len() {
        return Err(format!("cannot parse `{s}`: trailing tokens"));
    }
    Ok(e)
}

#[derive(Clone, Debug, PartialEq)]
enum V {
    B(bool),
    N(f64),
    S(String),
    /// Indeterminate: missing operand or unsupported predicate.
    U,
}

fn to_v(o: &Object) -> V {
    match o {
        Object::Boolean(b) => V::B(*b),
        Object::Integer(i) => V::N(*i as f64),
        Object::Real(r) => V::N(*r as f64),
        Object::Name(n) | Object::String(n, _) => V::S(String::from_utf8_lossy(n).into_owned()),
        Object::Array(_) | Object::Dictionary(_) | Object::Stream(_) => V::B(true),
        Object::Null | Object::Reference(_) => V::U,
    }
}

fn literal(a: &str) -> V {
    match a {
        "true" => V::B(true),
        "false" => V::B(false),
        _ => a.parse().map(V::N).unwrap_or_else(|_| V::S(a.to_string())),
    }
}

fn eq_literal(v: &V, lit: &str) -> bool {
    match (v, literal(lit)) {
        (V::N(x), V::N(y)) => (x - y).abs() < 1e-9,
        (V::S(x), _) => x == lit,
        (a, b) => *a == b,
    }
}

/// Evaluation context: the container holding the current key, its ancestors
/// (for `parent::`), the trailer (for `trailer::`) and the current key's value.
struct Cx<'a> {
    doc: &'a Document,
    trailer: &'a Object,
    size: usize,
    cur: &'a Object,
    parents: &'a [&'a Object],
    model: &'a Model,
    val: Option<&'a Object>,
}

fn dict_of(o: &Object) -> Option<&Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

impl<'a> Cx<'a> {
    fn deref(&self, o: &'a Object) -> Option<&'a Object> {
        self.doc.dereference(o).ok().map(|x| x.1).filter(|x| !matches!(x, Object::Null))
    }

    fn get(&self, o: &'a Object, seg: &str) -> Option<&'a Object> {
        if let Object::Array(a) = o {
            return a.get(seg.parse::<usize>().ok()?).and_then(|x| self.deref(x));
        }
        let d = dict_of(o)?;
        let seg = if seg == "Catalog" && std::ptr::eq(o, self.trailer) { "Root" } else { seg };
        d.get(seg.as_bytes()).ok().and_then(|x| self.deref(x))
    }

    fn resolve(&self, path: &str) -> Option<&'a Object> {
        let (mut cur, mut up) = (self.cur, self.parents.len());
        for s in path.split("::") {
            match s.trim_start_matches('@') {
                "trailer" => cur = self.trailer,
                "parent" => {
                    up = up.checked_sub(1)?;
                    cur = self.parents[up];
                }
                s => cur = self.get(cur, s)?,
            }
        }
        Some(cur)
    }

    fn value(&self, path: &str) -> V {
        match self.resolve(path) {
            Some(o) => to_v(o),
            None if !path.contains("::") => self
                .model
                .row(path.trim_start_matches('@'))
                .filter(|r| !r.default.is_empty() && !r.default.contains(['@', '[', '(']))
                .map(|r| literal(&r.default))
                .unwrap_or(V::U),
            None => V::U,
        }
    }

    fn key_path<'e>(&self, e: &'e E) -> Result<&'e str, String> {
        match e {
            E::Atom(a) => Ok(a.trim_start_matches('@')),
            _ => Err(format!("expected a key, got {e:?}")),
        }
    }

    fn bits(&self) -> Result<u32, String> {
        match self.val {
            Some(Object::Integer(i)) if (0..=u32::MAX as i64).contains(i) => Ok(*i as u32),
            v => Err(format!("bit predicate on non-32-bit integer {v:?}")),
        }
    }

    fn num(&self, e: &E) -> Result<f64, String> {
        match e {
            E::Atom(a) => a.parse().map_err(|_| format!("expected a number, got `{a}`")),
            _ => Err(format!("expected a number, got {e:?}")),
        }
    }

    fn ev(&self, e: &E) -> Result<V, String> {
        match e {
            // `@Key`, `parent::@Key`, `trailer::Catalog::@Key`: a value lookup.
            E::Atom(a) if a.rsplit("::").next().is_some_and(|s| s.starts_with('@')) => Ok(self.value(a)),
            E::Atom(a) => Ok(literal(a)),
            E::Bin(op, l, r) => {
                let (l, r) = (self.ev(l)?, self.ev(r)?);
                Ok(match (op.as_str(), &l, &r) {
                    ("&&", V::B(false), _) | ("&&", _, V::B(false)) => V::B(false),
                    ("&&", V::B(true), V::B(true)) => V::B(true),
                    ("||", V::B(true), _) | ("||", _, V::B(true)) => V::B(true),
                    ("||", V::B(false), V::B(false)) => V::B(false),
                    ("&&" | "||", _, _) => V::U,
                    (_, V::U, _) | (_, _, V::U) => V::U,
                    ("==", a, b) => V::B(a == b),
                    ("!=", a, b) => V::B(a != b),
                    (op, V::N(a), V::N(b)) => match op {
                        "<" => V::B(a < b),
                        "<=" => V::B(a <= b),
                        ">" => V::B(a > b),
                        ">=" => V::B(a >= b),
                        "+" => V::N(a + b),
                        "-" => V::N(a - b),
                        "*" => V::N(a * b),
                        "mod" => V::N(a % b),
                        _ => return Err(format!("unknown operator {op}")),
                    },
                    _ => return Err(format!("operator {op} on {l:?}, {r:?}")),
                })
            }
            E::Fn(name, args) => self.call(name, args),
        }
    }

    fn call(&self, name: &str, args: &[E]) -> Result<V, String> {
        let n = args.len();
        let ver = |i: usize| self.num(&args[i]);
        Ok(match (name, n) {
            ("fn:Eval" | "fn:IsRequired", 1) => self.ev(&args[0])?,
            ("fn:Not", 1) => match self.ev(&args[0])? {
                V::B(b) => V::B(!b),
                v => v,
            },
            ("fn:SinceVersion", 1) => V::B(PDF_VERSION >= ver(0)?),
            ("fn:SinceVersion", 2) if PDF_VERSION >= ver(0)? => self.ev(&args[1])?,
            ("fn:SinceVersion", 2) => V::B(false),
            ("fn:BeforeVersion", 1) => V::B(PDF_VERSION < ver(0)?),
            ("fn:BeforeVersion", 2) if PDF_VERSION < ver(0)? => self.ev(&args[1])?,
            ("fn:BeforeVersion", 2) => V::B(false),
            ("fn:IsPDFVersion", 1) => V::B(PDF_VERSION == ver(0)?),
            ("fn:IsPDFVersion", 2) if PDF_VERSION == ver(0)? => self.ev(&args[1])?,
            ("fn:IsPDFVersion", 2) => V::B(false),
            // No extensions are declared, so extension-only rules never apply.
            ("fn:Extension", 1 | 2) => V::B(false),
            ("fn:IsPresent", 1) => match &args[0] {
                E::Atom(a) if !a.starts_with('@') => V::B(self.resolve(a).is_some()),
                cond => self.ev(cond)?,
            },
            ("fn:IsPresent", 2) => match self.resolve(self.key_path(&args[0])?) {
                Some(_) => self.ev(&args[1])?,
                None => V::B(true),
            },
            ("fn:ArrayLength", 1) => match self.resolve(self.key_path(&args[0])?) {
                Some(Object::Array(a)) => V::N(a.len() as f64),
                Some(o) => return Err(format!("ArrayLength of non-array {o:?}")),
                None => V::U,
            },
            ("fn:StringLength", 1) => match self.resolve(self.key_path(&args[0])?) {
                Some(Object::String(s, _)) => V::N(s.len() as f64),
                Some(o) => return Err(format!("StringLength of non-string {o:?}")),
                None => V::U,
            },
            ("fn:Contains", 2) => {
                let lit = self.key_path(&args[1])?;
                match self.resolve(self.key_path(&args[0])?) {
                    Some(Object::Array(a)) => V::B(a.iter().any(|x| self.deref(x).is_some_and(|x| eq_literal(&to_v(x), lit)))),
                    Some(o) => V::B(eq_literal(&to_v(o), lit)),
                    None => V::B(false),
                }
            }
            ("fn:BitClear" | "fn:BitSet", 1) => {
                let set = self.bits()? >> (ver(0)? as u32 - 1) & 1 == 1;
                V::B(set == (name == "fn:BitSet"))
            }
            ("fn:BitsClear" | "fn:BitsSet", 2) => {
                let (lo, hi) = (ver(0)? as u32, ver(1)? as u32);
                let mask = ((1u64 << (hi - lo + 1)) - 1) as u32;
                let field = (self.bits()? as u64 >> (lo - 1)) as u32 & mask;
                V::B(if name == "fn:BitsSet" { field == mask } else { field == 0 })
            }
            ("fn:NotStandard14Font", 0) => {
                let d = dict_of(self.cur).ok_or("NotStandard14Font outside a dictionary")?;
                let is = |k: &[u8], v: &str| d.get(k).ok().and_then(|o| o.as_name().ok()) == Some(v.as_bytes());
                let base = d.get(b"BaseFont").ok().and_then(|o| o.as_name().ok()).unwrap_or_default();
                V::B(!(is(b"Type", "Font") && is(b"Subtype", "Type1") && STD14.iter().any(|s| s.as_bytes() == base)))
            }
            ("fn:FileSize", 0) => V::N(self.size as f64),
            // Strings can only be encrypted when the trailer has /Encrypt.
            ("fn:AlwaysUnencrypted", 0) if self.resolve("trailer::Encrypt").is_none() => V::B(true),
            // Exact when the document has no structure tree / name tree to look in.
            ("fn:InNameTree", 1) if self.resolve(self.key_path(&args[0])?).is_none() => V::B(false),
            ("fn:PageContainsStructContentItems" | "fn:ImageIsStructContentItem", 0)
                if self.resolve("trailer::Catalog::StructTreeRoot").is_none() =>
            {
                V::B(false)
            }
            _ => return Err(format!("unsupported predicate {name} with {n} argument(s)")),
        })
    }

    /// Evaluates a predicate cell that must hold; `Err` explains why it does not.
    fn holds(&self, src: &str) -> Result<(), String> {
        match self.ev(&parse(src)?)? {
            V::B(true) => Ok(()),
            V::B(false) => Err(format!("violates `{src}`")),
            v => Err(format!("cannot decide `{src}` (got {v:?})")),
        }
    }

    fn truth(&self, src: &str) -> Result<bool, String> {
        match src {
            "TRUE" => Ok(true),
            "FALSE" | "" => Ok(false),
            _ => match self.ev(&parse(src)?)? {
                V::B(b) => Ok(b),
                v => Err(format!("cannot decide `{src}` (got {v:?})")),
            },
        }
    }
}

fn numeric(o: &Object) -> bool {
    matches!(o, Object::Integer(_) | Object::Real(_))
}

fn type_ok(t: &str, o: &Object) -> bool {
    match (t, o) {
        ("boolean", Object::Boolean(_))
        | ("integer" | "bitmask", Object::Integer(_))
        | ("name", Object::Name(_))
        | ("string" | "string-byte" | "string-text", Object::String(..))
        | ("dictionary" | "name-tree" | "number-tree", Object::Dictionary(_))
        | ("stream", Object::Stream(_))
        | ("array", Object::Array(_))
        | ("null", Object::Null) => true,
        ("number", o) => numeric(o),
        ("string-ascii", Object::String(s, _)) => s.is_ascii(),
        ("date", Object::String(s, _)) => s.starts_with(b"D:"),
        ("rectangle", Object::Array(a)) => a.len() == 4 && a.iter().all(numeric),
        ("matrix", Object::Array(a)) => a.len() == 6 && a.iter().all(numeric),
        _ => false,
    }
}

struct Child<'a> {
    obj: &'a Object,
    id: Option<ObjectId>,
    cands: Vec<String>,
    path: String,
}

struct Walker<'a> {
    doc: &'a Document,
    models: &'a HashMap<String, Model>,
    trailer: &'a Object,
    size: usize,
    errs: Vec<String>,
    seen: HashSet<(ObjectId, String)>,
}

impl<'a> Walker<'a> {
    fn deref(&self, o: &'a Object) -> Option<&'a Object> {
        self.doc.dereference(o).ok().map(|x| x.1).filter(|x| !matches!(x, Object::Null))
    }

    fn visit(&mut self, c: Child<'a>, parents: &mut Vec<&'a Object>) {
        let vendored: Vec<&'a Model> = c.cands.iter().filter_map(|n| self.models.get(n)).collect();
        let model = match vendored.len() {
            0 => {
                self.errs.push(format!("{}: links to {:?}, none of which is vendored", c.path, c.cands));
                return;
            }
            1 => vendored[0],
            // Several possible models (e.g. page tree node vs page): the one the
            // object satisfies; if none does, the closest one reports its errors.
            _ => *vendored.iter().min_by_key(|m| self.check(c.obj, m, &c.path, parents).0.len()).unwrap(),
        };
        if let Some(id) = c.id {
            if !self.seen.insert((id, model.name.clone())) {
                return;
            }
        }
        let (errs, kids) = self.check(c.obj, model, &c.path, parents);
        self.errs.extend(errs);
        parents.push(c.obj);
        for k in kids {
            self.visit(k, parents);
        }
        parents.pop();
    }

    fn possible(&self, c: &Cx, pv: &str, val: &Object) -> Result<(), String> {
        let v = to_v(val);
        let mut ok = false;
        for item in split_top(pv, ',') {
            let item = item.trim();
            if item == "*" {
                ok = true;
            } else if let Some(inner) = item.strip_prefix("fn:RequiredValue(").and_then(|x| x.strip_suffix(')')) {
                let a = split_top(inner, ',');
                let [cond, req] = a[..] else { return Err(format!("malformed `{item}`")) };
                if eq_literal(&v, req.trim()) {
                    ok = true;
                } else if c.truth(cond)? {
                    return Err(format!("value {v:?} must be {req} because `{cond}`"));
                }
            } else if item.starts_with("fn:Eval(") {
                ok |= c.truth(item)?;
            } else if let Some(lit) = unwrap_ver(item) {
                if lit.starts_with(['[', '(']) || lit.starts_with("fn:") {
                    return Err(format!("unsupported possible value `{item}`"));
                }
                ok |= eq_literal(&v, lit);
            }
        }
        if ok { Ok(()) } else { Err(format!("value {v:?} not one of [{pv}]")) }
    }

    /// Checks one dictionary, stream or array against `model` without recursing;
    /// returns the errors and the linked children to visit.
    fn check(&self, obj: &'a Object, model: &'a Model, path: &str, parents: &[&'a Object]) -> (Vec<String>, Vec<Child<'a>>) {
        let (mut errs, mut kids) = (Vec::new(), Vec::new());
        let entries: Vec<(String, &'a Object)> = match obj {
            Object::Dictionary(d) => d.iter().map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), v)).collect(),
            Object::Stream(s) => s.dict.iter().map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), v)).collect(),
            Object::Array(a) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
            o => {
                errs.push(format!("{path}: {} needs a container, found {o:?}", model.name));
                return (errs, kids);
            }
        };
        let cx = |val| Cx { doc: self.doc, trailer: self.trailer, size: self.size, cur: obj, parents, model, val };
        let m = &model.name;
        for (key, raw) in &entries {
            let p = format!("{path}/{key}");
            let Some(row) = model.row(key) else {
                errs.push(format!("{p}: key not defined in {m}"));
                continue;
            };
            let Some(val) = self.deref(raw) else { continue }; // null == absent (7.3.7)
            if !since_version(&row.since).is_some_and(|v| v <= PDF_VERSION) {
                errs.push(format!("{p}: not in PDF 2.0 (SinceVersion `{}` in {m})", row.since));
                continue;
            }
            if row.deprecated.parse::<f64>().is_ok_and(|d| d <= PDF_VERSION) {
                errs.push(format!("{p}: deprecated in PDF {}", row.deprecated));
            }
            let types = split_top(&row.types, ';');
            let n = types.len();
            let Some(i) = types.iter().position(|t| unwrap_ver(t).is_some_and(|t| type_ok(t, val))) else {
                errs.push(format!("{p}: {val:?} is not of type `{}` ({m})", row.types));
                continue;
            };
            let c = cx(Some(val));
            let is_ref = matches!(raw, Object::Reference(_));
            match cell(&row.indirect, i, n) {
                "TRUE" if !is_ref => errs.push(format!("{p}: must be an indirect reference ({m})")),
                "TRUE" | "FALSE" => {}
                s => {
                    let (direct, arg) = match (s.strip_prefix("fn:MustBeDirect("), s.strip_prefix("fn:MustBeIndirect(")) {
                        (Some(a), _) => (true, a),
                        (_, Some(a)) => (false, a),
                        _ => (true, "\u{0}"),
                    };
                    let arg = arg.strip_suffix(')').unwrap_or("\u{0}");
                    match if arg.is_empty() { Ok(true) } else { c.truth(arg) } {
                        Ok(true) if is_ref == direct => {
                            errs.push(format!("{p}: must be {} ({m}: {s})", if direct { "direct" } else { "indirect" }))
                        }
                        Ok(_) => {}
                        Err(e) => errs.push(format!("{p}: IndirectReference `{s}`: {e}")),
                    }
                }
            }
            let pv = cell(&row.possible, i, n);
            if !pv.is_empty() {
                if let Err(e) = self.possible(&c, pv, val) {
                    errs.push(format!("{p}: {e} ({m})"));
                }
            }
            let sc = cell(&row.special, i, n);
            if let Some(cond) = sc.strip_prefix("fn:Ignore(").and_then(|x| x.strip_suffix(')')) {
                match if cond.is_empty() { Ok(true) } else { c.truth(cond) } {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(e) => errs.push(format!("{p}: {e} ({m})")),
                }
            } else if !sc.is_empty() {
                if let Err(e) = c.holds(sc) {
                    errs.push(format!("{p}: {e} ({m})"));
                }
            }
            if matches!(val, Object::Array(_) | Object::Dictionary(_) | Object::Stream(_)) {
                let ty = unwrap_ver(types[i]).unwrap_or_default();
                let cands: Vec<String> = split_top(cell(&row.link, i, n), ',')
                    .into_iter()
                    .filter_map(unwrap_ver)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                if !cands.is_empty() {
                    let id = if let Object::Reference(id) = raw { Some(*id) } else { None };
                    kids.push(Child { obj: val, id, cands, path: p });
                } else if !matches!(ty, "rectangle" | "matrix") {
                    errs.push(format!("{p}: type `{ty}` has no link to check its contents ({m})"));
                }
            }
        }
        let c = cx(None);
        for row in &model.rows {
            if row.key == "*" || !since_version(&row.since).is_some_and(|v| v <= PDF_VERSION) {
                continue;
            }
            if c.resolve(&row.key).is_some()
                || (row.inheritable && parents.iter().any(|p| dict_of(p).is_some_and(|d| d.has(row.key.as_bytes()))))
            {
                continue;
            }
            match c.truth(&row.required) {
                Ok(true) => errs.push(format!("{path}/{}: required by {m} but missing", row.key)),
                Ok(false) => {}
                Err(e) => errs.push(format!("{path}/{}: Required `{}`: {e}", row.key, row.required)),
            }
        }
        (errs, kids)
    }
}

/// Every Arlington violation found in `bytes`, walking from the trailer.
pub fn arlington_errors(bytes: &[u8]) -> Vec<String> {
    let models = load_models();
    let doc = match Document::load_mem(bytes) {
        Ok(d) => d,
        Err(e) => return vec![format!("lopdf cannot parse the file: {e}")],
    };
    let mut errs = Vec::new();
    if !bytes.starts_with(b"%PDF-2.0") || doc.version != "2.0" {
        errs.push(format!("header is not %PDF-2.0 (lopdf read {:?})", doc.version));
    }
    let trailer = Object::Dictionary(doc.trailer.clone());
    let mut w = Walker { doc: &doc, models: &models, trailer: &trailer, size: bytes.len(), errs, seen: HashSet::new() };
    let root = Child { obj: &trailer, id: None, cands: vec!["FileTrailer".into()], path: "trailer".into() };
    w.visit(root, &mut Vec::new());
    // Objects the walk never reached were never checked against any model.
    let reached: HashSet<ObjectId> = w.seen.iter().map(|(id, _)| *id).collect();
    for id in doc.objects.keys().filter(|id| !reached.contains(id)) {
        w.errs.push(format!("object {} {} R is not reachable from the trailer, so it was not checked", id.0, id.1));
    }
    w.errs
}

/// Panics with the full list when `bytes` has any Arlington or content-stream violation.
pub fn assert_conformant(name: &str, bytes: &[u8]) {
    let mut errs = arlington_errors(bytes);
    errs.extend(content::content_errors(bytes));
    assert!(errs.is_empty(), "{name}: {} conformance violation(s):\n  {}", errs.len(), errs.join("\n  "));
}
