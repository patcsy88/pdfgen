//! Content-stream checker: every operator against the ISO 32000-2 operator table
//! (Annex A, Table A.1) with its operand count and types, the graphics-object state
//! machine (8.2, Figure 9), q/Q and BT/ET and marked-content balance, resource names
//! (Tf, Do, gs, cs/CS, sh, BDC/DP) and that shown bytes are inside the font's
//! FirstChar..LastChar. The operator table was written from the standard's operator
//! summary as remembered, not copied from the normative text.

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Page,
    Text,
    Path,
    Clip,
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    /// General graphics state, colour, text state, marked content: page or text level.
    Any,
    /// q, Q, cm, Do, sh: page level only.
    PageOnly,
    PathStart,
    PathCont,
    Paint,
    Clip,
    Bt,
    Et,
    /// Text positioning and showing: inside BT only.
    InText,
    Compat,
}

/// Operand codes: n number, i integer, N name, s string, a array, D dict-or-name,
/// `*` 1-4 numbers (+ optional trailing name for `*N`).
const OPS: &[(&str, &str, K)] = &[
    ("w", "n", K::Any), ("J", "i", K::Any), ("j", "i", K::Any), ("M", "n", K::Any),
    ("d", "an", K::Any), ("ri", "N", K::Any), ("i", "n", K::Any), ("gs", "N", K::Any),
    ("q", "", K::PageOnly), ("Q", "", K::PageOnly), ("cm", "nnnnnn", K::PageOnly),
    ("m", "nn", K::PathStart), ("re", "nnnn", K::PathStart),
    ("l", "nn", K::PathCont), ("c", "nnnnnn", K::PathCont), ("v", "nnnn", K::PathCont),
    ("y", "nnnn", K::PathCont), ("h", "", K::PathCont),
    ("S", "", K::Paint), ("s", "", K::Paint), ("f", "", K::Paint), ("F", "", K::Paint),
    ("f*", "", K::Paint), ("B", "", K::Paint), ("B*", "", K::Paint), ("b", "", K::Paint),
    ("b*", "", K::Paint), ("n", "", K::Paint),
    ("W", "", K::Clip), ("W*", "", K::Clip),
    ("BT", "", K::Bt), ("ET", "", K::Et),
    ("Tc", "n", K::Any), ("Tw", "n", K::Any), ("Tz", "n", K::Any), ("TL", "n", K::Any),
    ("Tf", "Nn", K::Any), ("Tr", "i", K::Any), ("Ts", "n", K::Any),
    ("Td", "nn", K::InText), ("TD", "nn", K::InText), ("Tm", "nnnnnn", K::InText), ("T*", "", K::InText),
    ("Tj", "s", K::InText), ("'", "s", K::InText), ("\"", "nns", K::InText), ("TJ", "a", K::InText),
    ("CS", "N", K::Any), ("cs", "N", K::Any), ("SC", "*", K::Any), ("sc", "*", K::Any),
    ("SCN", "*N", K::Any), ("scn", "*N", K::Any), ("G", "n", K::Any), ("g", "n", K::Any),
    ("RG", "nnn", K::Any), ("rg", "nnn", K::Any), ("K", "nnnn", K::Any), ("k", "nnnn", K::Any),
    ("sh", "N", K::PageOnly), ("Do", "N", K::PageOnly),
    ("MP", "N", K::Any), ("DP", "ND", K::Any), ("BMC", "N", K::Any), ("BDC", "ND", K::Any), ("EMC", "", K::Any),
    ("BX", "", K::Compat), ("EX", "", K::Compat),
];

fn operand_ok(code: char, o: &Object) -> bool {
    match (code, o) {
        ('n', Object::Integer(_) | Object::Real(_)) | ('i', Object::Integer(_)) => true,
        ('N', Object::Name(_)) | ('s', Object::String(..)) | ('a', Object::Array(_)) => true,
        ('D', Object::Name(_) | Object::Dictionary(_)) => true,
        _ => false,
    }
}

fn operands_ok(spec: &str, ops: &[Object]) -> bool {
    let num = |o: &Object| operand_ok('n', o);
    match spec {
        "*" => (1..=4).contains(&ops.len()) && ops.iter().all(num),
        "*N" => match ops.split_last() {
            Some((Object::Name(_), rest)) => rest.len() <= 4 && rest.iter().all(num),
            _ => (1..=4).contains(&ops.len()) && ops.iter().all(num),
        },
        _ => spec.len() == ops.len() && spec.chars().zip(ops).all(|(c, o)| operand_ok(c, o)),
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Object> {
    doc.dereference(o).ok().map(|x| x.1)
}

fn dict<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    match deref(doc, o)? {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The page's Resources, inherited through /Parent when absent.
fn resources(doc: &Document, page: ObjectId) -> Option<&Dictionary> {
    let mut d = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Some(r) = d.get(b"Resources").ok().and_then(|r| dict(doc, r)) {
            return Some(r);
        }
        d = d.get(b"Parent").ok().and_then(|p| dict(doc, p))?;
    }
    None
}

fn name(o: &Object) -> &[u8] {
    o.as_name().unwrap_or_default()
}

/// Checks one decoded content stream sequence against the rules above.
fn check_ops(doc: &Document, res: Option<&Dictionary>, content: &[u8], at: &str) -> Vec<String> {
    let mut errs = Vec::new();
    let ops = match Content::decode(content) {
        Ok(c) => c.operations,
        Err(e) => return vec![format!("{at}: content stream does not parse: {e}")],
    };
    let lookup = |cat: &[u8], key: &[u8]| -> Option<&Dictionary> {
        res?.get(cat).ok().and_then(|c| dict(doc, c))?.get(key).ok().and_then(|o| dict(doc, o))
    };
    let (mut st, mut q, mut marked, mut compat) = (St::Page, Vec::new(), 0usize, 0usize);
    // Current font's (FirstChar, LastChar); graphics state, so saved by q / restored by Q.
    let mut font: Option<(i64, i64)> = None;
    for (n, op) in ops.iter().enumerate() {
        let o = op.operator.as_str();
        let here = format!("{at}: op {n} `{o}`");
        let Some(&(_, spec, kind)) = OPS.iter().find(|e| e.0 == o) else {
            if compat == 0 {
                errs.push(format!("{here}: not a PDF 2.0 content operator (BI/ID/EI inline images and d0/d1 are not expected here)"));
            }
            continue;
        };
        if !operands_ok(spec, &op.operands) {
            errs.push(format!("{here}: operands {:?} do not match `{spec}`", op.operands));
            continue;
        }
        let ok = match (kind, st) {
            (K::Compat, _) => true,
            (K::Any, St::Page | St::Text) | (K::PageOnly | K::Bt, St::Page) => true,
            (K::PathStart, St::Page | St::Path) | (K::PathCont | K::Clip, St::Path) => true,
            (K::Paint, St::Path | St::Clip) | (K::Et | K::InText, St::Text) => true,
            _ => false,
        };
        if !ok {
            errs.push(format!("{here}: not allowed in {st:?} state (ISO 32000-2 Figure 9)"));
            continue;
        }
        st = match kind {
            K::PathStart | K::PathCont => St::Path,
            K::Clip => St::Clip,
            K::Paint | K::Et => St::Page,
            K::Bt => St::Text,
            _ => st,
        };
        let arg = op.operands.first().map(name).unwrap_or_default();
        let need = |cat: &[u8]| lookup(cat, arg).is_none().then(|| {
            format!("{here}: /{} is not in Resources /{}", String::from_utf8_lossy(arg), String::from_utf8_lossy(cat))
        });
        if matches!(o, "BMC" | "BDC") {
            marked += 1;
        }
        match o {
            "q" => q.push(font),
            "Q" => match q.pop() {
                Some(f) => font = f,
                None => errs.push(format!("{here}: Q without matching q")),
            },
            "EMC" if marked == 0 => errs.push(format!("{here}: EMC without BMC/BDC")),
            "EMC" => marked -= 1,
            "BX" => compat += 1,
            "EX" => compat = compat.saturating_sub(1),
            "Tf" => {
                errs.extend(need(b"Font"));
                font = lookup(b"Font", arg).map(|f| {
                    let int = |k: &[u8]| f.get(k).ok().and_then(|o| o.as_i64().ok());
                    (int(b"FirstChar").unwrap_or(0), int(b"LastChar").unwrap_or(255))
                });
            }
            "Do" => errs.extend(need(b"XObject")),
            "gs" => errs.extend(need(b"ExtGState")),
            "sh" => errs.extend(need(b"Shading")),
            "CS" | "cs" if !matches!(arg, b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK" | b"Pattern") => {
                errs.extend(need(b"ColorSpace"))
            }
            "DP" | "BDC" if matches!(op.operands.get(1), Some(Object::Name(_))) => {
                let p = name(&op.operands[1]);
                if lookup(b"Properties", p).is_none() {
                    errs.push(format!("{here}: /{} is not in Resources /Properties", String::from_utf8_lossy(p)));
                }
            }
            _ => {}
        }
        if matches!(o, "Tj" | "'" | "\"" | "TJ") {
            let Some((first, last)) = font else {
                errs.push(format!("{here}: text shown before Tf selected a font"));
                continue;
            };
            let strings: Vec<&[u8]> = match &op.operands[..] {
                [.., Object::String(s, _)] => vec![s],
                [Object::Array(a)] => a.iter().filter_map(|x| if let Object::String(s, _) = x { Some(&s[..]) } else { None }).collect(),
                _ => vec![],
            };
            if let Object::Array(a) = &op.operands[0] {
                if o == "TJ" && !a.iter().all(|x| matches!(x, Object::String(..) | Object::Integer(_) | Object::Real(_))) {
                    errs.push(format!("{here}: TJ array may hold only strings and numbers"));
                }
            }
            for b in strings.iter().flat_map(|s| s.iter()) {
                if !(first..=last).contains(&(*b as i64)) {
                    errs.push(format!("{here}: byte {b} outside font FirstChar..LastChar {first}..{last}"));
                }
            }
        }
    }
    let end = [
        (st != St::Page, format!("ends in {st:?} state (unterminated text object or path)")),
        (!q.is_empty(), format!("{} q without matching Q", q.len())),
        (marked != 0, format!("{marked} BMC/BDC without EMC")),
        (compat != 0, format!("{compat} BX without EX")),
    ];
    errs.extend(end.into_iter().filter(|e| e.0).map(|e| format!("{at}: {}", e.1)));
    errs
}

/// Content-stream violations on every page of `bytes`.
pub fn content_errors(bytes: &[u8]) -> Vec<String> {
    let doc = match Document::load_mem(bytes) {
        Ok(d) => d,
        Err(e) => return vec![format!("lopdf cannot parse the file: {e}")],
    };
    let mut errs = Vec::new();
    for (no, id) in doc.get_pages() {
        let at = format!("page {no}");
        match doc.get_page_content(id) {
            Ok(c) => errs.extend(check_ops(&doc, resources(&doc, id), &c, &at)),
            Err(e) => errs.push(format!("{at}: cannot read /Contents: {e}")),
        }
    }
    errs
}
