//! Seeded random-input tests for `pdfgen_core`.
//!
//! Not coverage-guided: a splitmix64 generator builds random documents (text with awkward characters,
//! tables with zero/oversized/extra cells, raw and JPEG images, page breaks, and floats such as NaN,
//! infinity, zero and negatives) and mutated JPEG headers. For every input `render` must not panic, must
//! return either `Ok` or one of the documented errors, and every `Ok` file must pass the Arlington and
//! content-stream checkers in `tests/common`. Every `QPDF_EVERY`-th file also goes through `qpdf --check`.
//!
//! A failure prints the seed and iteration. Reproduce with `PDFGEN_FUZZ_SEED=<seed> PDFGEN_FUZZ_ITERS=<n>`.
//! Defaults: seed 1, 300 documents, 3000 JPEG mutations (each accepted one is rendered, every 10th is fully
//! checked). Raise `PDFGEN_FUZZ_ITERS` for a longer run.

mod common;

use pdfgen_core::{Block, Document, Error, Font, Image, Table};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::Command;

const QPDF_EVERY: u64 = 20;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`; `n` must be > 0.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

const EDGE_FLOATS: [f32; 12] = [
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    0.0,
    -0.0,
    -1.0,
    -100.0,
    1e-30,
    f32::MIN_POSITIVE,
    1e30,
    f32::MAX,
    f32::EPSILON,
];

/// Mostly ordinary values in `lo..hi`, sometimes an awkward one.
fn float(r: &mut Rng, lo: f32, hi: f32) -> f32 {
    if r.chance(6) {
        *r.pick(&EDGE_FLOATS)
    } else {
        lo + (hi - lo) * (r.below(10_001) as f32 / 10_000.0)
    }
}

const FONTS: [Font; 4] = Font::ALL;

const CHAR_POOL: [char; 22] = [
    'a', 'b', 'Z', '0', ' ', ' ', '(', ')', '\\', '\n', '\r', '\t', '\0', '\u{7f}', '\u{85}', '\u{e9}', '\u{ff}', '\u{a0}', '\u{20ac}',
    '\u{1f600}', '\u{200d}', '\u{fffd}',
];

fn text(r: &mut Rng) -> String {
    let n = match r.below(10) {
        0 => 0,
        1..=6 => r.below(60),
        7 | 8 => r.below(500),
        _ => r.below(3000),
    };
    let word_len = if r.chance(15) { 1 + r.below(400) } else { 0 }; // long unbroken words force hard breaks
    let mut s = String::new();
    let mut run = 0;
    for _ in 0..n {
        if word_len > 0 && run < word_len {
            s.push('w');
            run += 1;
        } else {
            s.push(*r.pick(&CHAR_POOL));
            run = 0;
        }
    }
    s
}

/// Header-only JPEG: SOI, SOF`marker`, SOS, EOI. `parse_jpeg` stops at SOS.
fn jpeg(marker: u8, w: u16, h: u16, comps: u8) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, marker];
    b.extend_from_slice(&(8 + 3 * comps as u16).to_be_bytes());
    b.push(8);
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&w.to_be_bytes());
    b.push(comps);
    for i in 0..comps {
        b.extend_from_slice(&[i + 1, 0x11, 0]);
    }
    b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
    b
}

/// A random image and whether it is a header-only fake JPEG.
fn image(r: &mut Rng) -> Result<(Image, bool), Error> {
    match r.below(3) {
        0 | 1 => {
            let (w, h) = (1 + r.below(24) as u32, 1 + r.below(24) as u32);
            let ch = if r.chance(50) { 3 } else { 1 };
            let mut len = (w * h * ch) as usize;
            if r.chance(10) {
                len = len.wrapping_add(r.below(5) as usize).wrapping_sub(2); // sometimes wrong
            }
            let data: Vec<u8> = (0..len).map(|_| r.below(256) as u8).collect();
            (if ch == 3 { Image::rgb(w, h, data) } else { Image::gray(w, h, data) }).map(|i| (i, false))
        }
        _ => {
            let comps = *r.pick(&[1u8, 1, 3, 3, 3, 0, 2, 4]);
            let marker = *r.pick(&[0xC0u8, 0xC1, 0xC2, 0xC0]);
            // Aspect ratios far from 1 exercise the clamp to the page height.
            let (w, h) = (*r.pick(&[1u16, 7, 300, 4000, 65535, 0]), *r.pick(&[1u16, 9, 600, 4000, 65535, 0]));
            Image::jpeg(jpeg(marker, w, h, comps)).map(|i| (i, true))
        }
    }
}

fn table(r: &mut Rng) -> Table {
    // Malformed shapes are decided once per table: 5% no columns, 8% one row with too many cells. A per-row
    // chance would make nearly every multi-row table fail validation before reaching the layout code.
    let ncols = if r.chance(5) { 0 } else { 1 + r.below(5) as usize };
    let too_wide_row = r.chance(8);
    let column_widths: Vec<f32> = (0..ncols)
        .map(|_| match r.below(8) {
            0 => r.below(12) as f32, // narrower than the cell padding
            1 => 1.0e4,
            _ => float(r, 10.0, 200.0),
        })
        .collect();
    let row = |r: &mut Rng, extra: usize| -> Vec<String> {
        let n = r.below(ncols as u64 + 1) as usize + extra;
        // Mostly short cells; a long one is the usual cause of RowTooTall, which is worth hitting but rarely.
        (0..n).map(|_| if r.chance(4) { text(r) } else { text(r).chars().take(r.below(60) as usize).collect() }).collect()
    };
    let header = if r.chance(60) { Some(row(r, 0)) } else { None };
    let nrows = r.below(40);
    let bad_at = r.below(nrows.max(1));
    let rows: Vec<Vec<String>> = (0..nrows).map(|i| row(r, (too_wide_row && i == bad_at) as usize * (ncols + 1))).collect();
    Table { column_widths, header, rows, font_size: float(r, 4.0, 24.0) }
}

/// A random document, a description of it for failure messages, and whether it holds a header-only fake
/// JPEG (whose pixel data qpdf cannot decode).
fn document(r: &mut Rng) -> (Document, Vec<String>, bool) {
    let mut d = Document::new();
    let mut log = Vec::new();
    let mut fake_jpeg = false;
    for _ in 0..1 + r.below(8) {
        match r.below(5) {
            0 | 1 => {
                let (t, f, s) = (text(r), *r.pick(&FONTS), float(r, 4.0, 40.0));
                log.push(format!("text len={} font={f:?} size={s:?}", t.chars().count()));
                d.add(Block::Text { text: t, font: f, size: s });
            }
            2 => {
                let t = table(r);
                log.push(format!(
                    "table widths={:?} header={} rows={} size={:?}",
                    t.column_widths,
                    t.header.is_some(),
                    t.rows.len(),
                    t.font_size
                ));
                d.add(Block::Table(t));
            }
            3 => match image(r) {
                Ok((img, jpeg)) => {
                    let w = float(r, 1.0, 600.0);
                    log.push(format!("image {}x{} jpeg={jpeg} width={w:?}", img.width(), img.height()));
                    fake_jpeg |= jpeg;
                    d.add(Block::Image { image: img, width: w });
                }
                Err(e) => assert!(
                    matches!(e, Error::BadImageData(_) | Error::BadJpeg(_)),
                    "image constructor returned an undocumented error: {e:?}"
                ),
            },
            _ => {
                log.push("page break".into());
                d.add(Block::PageBreak);
            }
        }
    }
    (d, log, fake_jpeg)
}

fn qpdf_check(bytes: &[u8], tag: &str) -> Result<(), String> {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fuzz-{tag}.pdf"));
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new("qpdf")
        .args(["--check", path.to_str().unwrap()])
        .output()
        .expect("qpdf must be installed to run these tests (apt install qpdf)");
    let _ = std::fs::remove_file(&path);
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }
}

#[test]
fn random_documents_render_to_valid_pdfs_or_documented_errors() {
    let seed = env_u64("PDFGEN_FUZZ_SEED", 1);
    let iters = env_u64("PDFGEN_FUZZ_ITERS", 300);
    let (mut ok, mut rejected) = (0u64, 0u64);
    let mut by_error: std::collections::BTreeMap<&'static str, u64> = std::collections::BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    for i in 0..iters {
        let mut r = Rng(seed.wrapping_add(i.wrapping_mul(0xD134_2543_DE82_EF95)));
        let (doc, log, fake_jpeg) = document(&mut r);
        let ctx = format!("seed={seed} iter={i}: [{}]", log.join("; "));
        match catch_unwind(AssertUnwindSafe(|| doc.render())) {
            Err(_) => failures.push(format!("{ctx}: render panicked")),
            Ok(Err(e)) => {
                rejected += 1;
                *by_error.entry(match e {
                    Error::BadTable(_) => "BadTable",
                    Error::RowTooTall => "RowTooTall",
                    Error::BadDimension(_) => "BadDimension",
                    Error::BadImageData(_) => "BadImageData",
                    Error::BadJpeg(_) => "BadJpeg",
                    Error::TooLarge(_) => "TooLarge",
                }).or_default() += 1;
            }
            Ok(Ok(bytes)) => {
                ok += 1;
                let mut errs = common::arlington_errors(&bytes);
                errs.extend(common::content::content_errors(&bytes));
                if i % QPDF_EVERY == 0 && !fake_jpeg {
                    if let Err(e) = qpdf_check(&bytes, &format!("{seed}-{i}")) {
                        errs.push(format!("qpdf --check failed: {}", e.trim()));
                    }
                }
                if !errs.is_empty() {
                    let shown: Vec<_> = errs.iter().take(3).cloned().collect();
                    failures.push(format!("{ctx}: {} violation(s), first: {}", errs.len(), shown.join(" | ")));
                }
            }
        }
    }
    eprintln!("fuzz documents: seed={seed} iters={iters} rendered={ok} rejected={rejected} {by_error:?} failed={}", failures.len());
    assert!(failures.is_empty(), "{} of {iters} documents failed:\n{}", failures.len(), failures.join("\n"));
    assert!(ok > iters / 4, "generator is too hostile: only {ok} of {iters} documents rendered");
}

#[test]
fn mutated_jpeg_headers_never_panic_and_embed_validly() {
    let seed = env_u64("PDFGEN_FUZZ_SEED", 1);
    let iters = env_u64("PDFGEN_FUZZ_ITERS", 300) * 10;
    let mut accepted = 0u64;
    let mut failures: Vec<String> = Vec::new();
    for i in 0..iters {
        let mut r = Rng(seed ^ 0xA5A5_A5A5_0000_0000u64.wrapping_add(i.wrapping_mul(0x9E37_79B9)));
        let mut b = jpeg(*r.pick(&[0xC0u8, 0xC2]), 1 + r.below(2000) as u16, 1 + r.below(2000) as u16, *r.pick(&[1u8, 3]));
        match r.below(4) {
            0 => {
                for _ in 0..1 + r.below(4) {
                    let at = r.below(b.len() as u64) as usize;
                    b[at] = r.below(256) as u8;
                }
            }
            1 => b.truncate(r.below(b.len() as u64 + 1) as usize),
            2 => {
                let at = r.below(b.len() as u64 + 1) as usize;
                let junk: Vec<u8> = (0..r.below(12)).map(|_| r.below(256) as u8).collect();
                b.splice(at..at, junk);
            }
            _ => b = (0..r.below(40)).map(|_| r.below(256) as u8).collect(),
        }
        let ctx = format!("seed={seed} iter={i} bytes={b:02x?}");
        let built = match catch_unwind(AssertUnwindSafe(|| Image::jpeg(b.clone()))) {
            Err(_) => {
                failures.push(format!("{ctx}: Image::jpeg panicked"));
                continue;
            }
            Ok(Err(Error::BadJpeg(_))) => continue,
            Ok(Err(e)) => {
                failures.push(format!("{ctx}: undocumented error {e:?}"));
                continue;
            }
            Ok(Ok(img)) => img,
        };
        accepted += 1;
        let mut d = Document::new();
        d.add_image(built, 1.0 + r.below(500) as f32);
        match catch_unwind(AssertUnwindSafe(|| d.render())) {
            Err(_) => failures.push(format!("{ctx}: render panicked")),
            Ok(Err(e)) => failures.push(format!("{ctx}: render failed {e:?}")),
            Ok(Ok(bytes)) => {
                // Every accepted header is rendered; the slow full conformance walk covers every 10th, since
                // these files differ from each other only in the image's width, height and component count.
                if accepted % 10 == 1 {
                    let mut errs = common::arlington_errors(&bytes);
                    errs.extend(common::content::content_errors(&bytes));
                    if !errs.is_empty() {
                        failures.push(format!("{ctx}: {} violation(s), first: {}", errs.len(), errs[0]));
                    }
                }
            }
        }
    }
    eprintln!("fuzz jpeg: seed={seed} iters={iters} accepted={accepted} failed={}", failures.len());
    assert!(failures.is_empty(), "{} of {iters} inputs failed:\n{}", failures.len(), failures.join("\n"));
    assert!(accepted > 0, "no mutated header was accepted; the mutations are too destructive to test embedding");
}
