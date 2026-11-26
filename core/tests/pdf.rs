mod common;

use pdfgen_core::{Block, Document, Error, Font, Image, Table};
use std::path::PathBuf;
use std::process::Command;

/// Generated PDFs land in `target/test-pdfs/` so external validators
/// (`tools/validate-pdfcpu.sh`) can run over exactly what the tests checked.
fn out(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/test-pdfs");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn qpdf(args: &[&str]) -> std::process::Output {
    Command::new("qpdf")
        .args(args)
        .output()
        .expect("qpdf must be installed to run these tests (apt install qpdf)")
}

/// Writes the PDF, requires `qpdf --check` to exit 0 and zero Arlington-model
/// (ISO 32000-2) violations, returns the path.
fn write_and_check(name: &str, doc: &Document) -> PathBuf {
    let path = out(name);
    let bytes = doc.render().unwrap();
    std::fs::write(&path, &bytes).unwrap();
    common::assert_conformant(name, &bytes);
    let r = qpdf(&["--check", path.to_str().unwrap()]);
    assert!(
        r.status.success(),
        "qpdf --check failed for {name}: {}{}",
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    );
    path
}

fn page_count(p: &PathBuf) -> usize {
    let r = qpdf(&["--show-npages", p.to_str().unwrap()]);
    assert!(r.status.success());
    String::from_utf8_lossy(&r.stdout).trim().parse().unwrap()
}

/// Decompressed (QDF) text of the file, for content assertions.
fn qdf(p: &PathBuf) -> Vec<u8> {
    let q = p.with_extension("qdf.pdf"); // unique per input: tests run in parallel
    let r = qpdf(&["--qdf", "--object-streams=disable", p.to_str().unwrap(), q.to_str().unwrap()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    std::fs::read(q).unwrap()
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn text_is_wrapped_and_present() {
    let mut d = Document::new();
    d.add_text("Quarterly Report", Font::HelveticaBold, 20.0);
    let long = "lorem ipsum dolor sit amet ".repeat(40);
    d.add_text(&long, Font::Helvetica, 11.0);
    d.add_text("escapes: (parens) and back\\slash, caf\u{e9}", Font::Helvetica, 11.0);
    let p = write_and_check("text.pdf", &d);
    let q = qdf(&p);
    assert!(contains(&q, "(Quarterly Report) Tj"));
    assert!(contains(&q, "\\(parens\\) and back\\\\slash"));
    // qpdf --qdf normalises the "\351" escape to the raw WinAnsi byte 0xE9.
    assert!(q.windows(4).any(|w| w == b"caf\xE9"));
    // The long paragraph must be split over several Tj lines.
    let tj = String::from_utf8_lossy(&q).matches("lorem ipsum").count();
    assert!(tj >= 3, "expected wrapped lines, got {tj}");
    assert_eq!(page_count(&p), 1);
}

#[test]
fn nbsp_is_kept_inside_the_word() {
    let mut d = Document::new();
    d.add_text("a\u{a0}b", Font::Helvetica, 12.0);
    // 100 x "ab" joined by NBSP is far wider than a line: it must be hard-broken by characters, not at NBSP.
    let long = vec!["ab"; 100].join("\u{a0}");
    d.add_text(&long, Font::Courier, 12.0);
    let q = qdf(&write_and_check("nbsp.pdf", &d));
    // qpdf --qdf re-serialises the strings as hex: `(a\240b)` becomes `<61a062>`.
    assert!(contains(&q, "<61a062> Tj"), "NBSP must be written as byte 0xA0 (\\240)");
    assert!(!contains(&q, "<612062>"), "NBSP turned into a space");
    // Courier 12pt: 0.6 * 12 = 7.2pt per char, 495pt line -> 68 chars per line, not 2 ("ab").
    assert!(!contains(&q, "<6162> Tj"), "phrase broke at NBSP");
    assert!(contains(&q, "<6162a06162a0"), "long phrase lines keep NBSP between words");
}

#[test]
fn all_four_fonts_are_declared_and_used() {
    let mut d = Document::new();
    d.add_text("sans", Font::Helvetica, 12.0)
        .add_text("sans bold", Font::HelveticaBold, 12.0)
        .add_text("mono", Font::Courier, 12.0)
        .add_text("mono bold", Font::CourierBold, 12.0);
    let p = write_and_check("fonts.pdf", &d);
    let q = qdf(&p);
    for name in ["/BaseFont /Helvetica\n", "/BaseFont /Helvetica-Bold", "/BaseFont /Courier\n", "/BaseFont /Courier-Bold"] {
        assert!(contains(&q, name), "missing {name}");
    }
    for f in ["/F1 12 Tf", "/F2 12 Tf", "/F3 12 Tf", "/F4 12 Tf"] {
        assert!(contains(&q, f), "font not selected: {f}");
    }
}

#[test]
fn font_metrics_known_values() {
    // AFM: Helvetica "A"=667, " "=278; Courier is fixed 600.
    assert_eq!(Font::Helvetica.text_width("A", 1000.0), 667.0);
    assert_eq!(Font::Helvetica.text_width(" ", 1000.0), 278.0);
    assert_eq!(Font::HelveticaBold.text_width("A", 1000.0), 722.0);
    assert_eq!(Font::Courier.text_width("iii", 1000.0), 1800.0);
}

fn sample_table(rows: usize) -> Table {
    Table {
        column_widths: vec![60.0, 200.0, 100.0],
        header: Some(vec!["ID".into(), "Description".into(), "Amount".into()]),
        rows: (0..rows)
            .map(|i| vec![format!("R{i:03}"), format!("Line item number {i} with a fairly long description"), format!("{}.00", i * 7)])
            .collect(),
        font_size: 10.0,
    }
}

#[test]
fn table_renders_cells_and_borders() {
    let mut d = Document::new();
    d.add_table(sample_table(3));
    let p = write_and_check("table.pdf", &d);
    let q = qdf(&p);
    assert!(contains(&q, "(Description) Tj"));
    assert!(contains(&q, "(R002) Tj"));
    // 3 header + 9 body cells, each stroked once.
    assert_eq!(String::from_utf8_lossy(&q).matches(" re S").count(), 12);
    // Header shading drawn for 3 header cells.
    assert_eq!(String::from_utf8_lossy(&q).matches(" re f").count(), 3);
}

#[test]
fn table_wraps_long_cell_text_inside_column() {
    let mut d = Document::new();
    d.add_table(Table {
        column_widths: vec![80.0],
        header: None,
        rows: vec![vec!["alpha beta gamma delta epsilon zeta eta theta".into()]],
        font_size: 10.0,
    });
    let p = write_and_check("table_wrap.pdf", &d);
    let q = String::from_utf8_lossy(&qdf(&p)).to_string();
    assert!(q.matches(") Tj").count() >= 4, "cell must wrap to several lines");
}

#[test]
fn table_splits_across_pages_and_repeats_header() {
    let mut d = Document::new();
    d.add_table(sample_table(120));
    let p = write_and_check("table_pages.pdf", &d);
    let pages = page_count(&p);
    assert!(pages >= 3, "120 two-line rows must span pages, got {pages}");
    let q = String::from_utf8_lossy(&qdf(&p)).to_string();
    assert_eq!(q.matches("(Description) Tj").count(), pages, "header repeats on every page");
    assert!(q.contains("(R119) Tj"));
}

#[test]
fn row_taller_than_page_under_header_is_rejected() {
    // Header (6 wrapped lines) plus a 60-line row exceed the text area; old code drew the row below the
    // bottom margin after repeating the header on a fresh page.
    let mut d = Document::new();
    d.add_text("x", Font::Helvetica, 10.0);
    d.add_table(Table {
        column_widths: vec![30.0],
        header: Some(vec!["h1 h2 h3 h4 h5 h6".into()]),
        rows: vec![vec!["x".into()], vec![vec!["y"; 60].join("\n")]],
        font_size: 10.0,
    });
    assert_eq!(d.render().unwrap_err(), Error::RowTooTall);
}

#[test]
fn header_and_row_filling_the_page_stay_inside_margins() {
    // One-line header (20pt) + 59-line row (716pt) = 736pt <= 742pt text area.
    let table = Table {
        column_widths: vec![60.0],
        header: Some(vec!["H".into()]),
        rows: vec![vec!["x".into()], vec![vec!["y"; 59].join("\n")]],
        font_size: 10.0,
    };
    let mut d = Document::new();
    d.add_text("x", Font::Helvetica, 10.0);
    d.add_table(table.clone());
    d.add_table(table); // second copy: header on a partly filled page must move with its row
    let p = write_and_check("table_header_fill.pdf", &d);
    let q = String::from_utf8_lossy(&qdf(&p)).to_string();
    let mut rects = 0;
    for line in q.lines().filter(|l| l.ends_with(" re S")) {
        let nums: Vec<f32> = line.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        let (y, h) = (nums[nums.len() - 3], nums[nums.len() - 1]);
        assert!(y >= 50.0 - 0.01 && y + h <= 842.0 - 50.0 + 0.01, "rect outside margins: {line}");
        rects += 1;
    }
    assert!(rects >= 8, "expected header and row rects, got {rects}");
}

#[test]
fn table_errors() {
    let mut d = Document::new();
    d.add_table(Table { column_widths: vec![], header: None, rows: vec![], font_size: 10.0 });
    assert!(matches!(d.render(), Err(Error::BadTable(_))));

    let mut d = Document::new();
    d.add_table(Table {
        column_widths: vec![50.0],
        header: None,
        rows: vec![vec!["a".into(), "b".into()]],
        font_size: 10.0,
    });
    assert!(matches!(d.render(), Err(Error::BadTable(_))));

    let mut d = Document::new();
    d.add_table(Table {
        column_widths: vec![30.0],
        header: None,
        rows: vec![vec!["word ".repeat(2000)]],
        font_size: 10.0,
    });
    assert_eq!(d.render().unwrap_err(), Error::RowTooTall);
}

#[test]
fn non_finite_or_out_of_range_dimensions_are_rejected_not_written() {
    // Each of these used to render a PDF whose content stream held `NaN`, `inf`, a 1e30 coordinate or a
    // negative scale (found by tests/fuzz.rs); qpdf accepted some of them, content-stream rules did not.
    let table = |widths: Vec<f32>, size: f32| Table { column_widths: widths, header: None, rows: vec![vec!["a".into()]], font_size: size };
    let bad_text = |size: f32| {
        let mut d = Document::new();
        d.add_text("x", Font::Helvetica, size);
        d.render()
    };
    for size in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -0.0, -1.0, 1000.5, 1e30, f32::MAX] {
        assert!(matches!(bad_text(size), Err(Error::BadDimension(_))), "text size {size}");
    }
    for (widths, size) in [
        (vec![50.0, f32::NAN], 10.0),
        (vec![f32::INFINITY], 10.0),
        (vec![0.0], 10.0),
        (vec![-100.0], 10.0),
        (vec![20000.0, 20000.0], 10.0),
        (vec![50.0], f32::NAN),
        (vec![50.0], -0.0),
    ] {
        let mut d = Document::new();
        d.add_table(table(widths.clone(), size));
        assert!(matches!(d.render(), Err(Error::BadDimension(_))), "table {widths:?} size {size}");
    }
    for width in [f32::NAN, f32::INFINITY, 0.0, -50.0] {
        let mut d = Document::new();
        d.add_image(Image::gray(2, 2, vec![0, 1, 2, 3]).unwrap(), width);
        assert!(matches!(d.render(), Err(Error::BadDimension(_))), "image width {width}");
    }

    // The limits themselves are accepted and still conformant.
    let mut d = Document::new();
    d.add_text("big", Font::Helvetica, 1000.0).add_text("tiny", Font::Courier, 1e-3);
    d.add_table(table(vec![32000.0], 1.0));
    d.add_image(Image::gray(2, 2, vec![0, 1, 2, 3]).unwrap(), 1e30);
    write_and_check("dimension_limits.pdf", &d);
}

fn gradient(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 128]);
        }
    }
    v
}

#[test]
fn rgb_and_gray_images_are_embedded_flate() {
    let rgb = Image::rgb(16, 8, gradient(16, 8)).unwrap();
    let gray = Image::gray(4, 4, vec![0, 64, 128, 255].repeat(4)).unwrap();
    let mut d = Document::new();
    d.add_image(rgb, 160.0).add_image(gray, 40.0);
    let p = write_and_check("images.pdf", &d);
    let q = qdf(&p);
    assert!(contains(&q, "/Width 16"));
    assert!(contains(&q, "/Height 8"));
    assert!(contains(&q, "/ColorSpace /DeviceRGB"));
    assert!(contains(&q, "/ColorSpace /DeviceGray"));
    assert!(contains(&q, "/Subtype /Image"));
    // 160pt wide, aspect 2:1 -> 80pt tall.
    assert!(contains(&q, "q 160.00 0 0 80.00"));
    // qpdf decodes the Flate stream: its length must equal the raw pixel count.
    // Object layout: 1 catalog, 2 pages, 3-6 fonts, 7-10 font descriptors, 11 = first image.
    let r = qpdf(&["--show-object=11", "--filtered-stream-data", p.to_str().unwrap()]);
    assert!(r.status.success());
    assert_eq!(r.stdout.len(), 16 * 8 * 3, "decoded image bytes must round-trip");
    assert_eq!(r.stdout, gradient(16, 8));
}

#[test]
fn same_image_used_twice_is_stored_once() {
    let img = Image::gray(2, 2, vec![0, 85, 170, 255]).unwrap();
    let mut d = Document::new();
    d.add(Block::Image { image: img.clone(), width: 50.0 });
    d.add(Block::Image { image: img, width: 50.0 });
    let p = write_and_check("two_images.pdf", &d);
    // Distinct Image values are distinct XObjects (no content hashing): 2 here.
    assert_eq!(String::from_utf8_lossy(&qdf(&p)).matches("/Subtype /Image").count(), 2);
}

#[test]
fn image_validation() {
    assert!(matches!(Image::rgb(2, 2, vec![0; 11]), Err(Error::BadImageData(_))));
    assert!(matches!(Image::gray(0, 5, vec![]), Err(Error::BadImageData(_))));
    assert!(matches!(Image::jpeg(b"not a jpeg".to_vec()), Err(Error::BadJpeg(_))));
}

/// SOI + SOF0 header only. Exercises dimension/colour parsing and DCTDecode
/// embedding; qpdf does not decode DCT data, so pixel validity is NOT tested.
fn jpeg_header(w: u16, h: u16, comps: u8) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 8 + 3 * comps, 8];
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&w.to_be_bytes());
    v.push(comps);
    for c in 0..comps {
        v.extend_from_slice(&[c + 1, 0x11, 0]);
    }
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

/// Minimal valid baseline grayscale JPEG (flat mid-gray), `w` and `h` multiples
/// of 8. One 8x8 block = DC code "0" + EOB code "0", each a 1-bit Huffman code.
fn gray_jpeg(w: u16, h: u16) -> Vec<u8> {
    assert!(w % 8 == 0 && h % 8 == 0);
    let blocks = (w as usize / 8) * (h as usize / 8);
    let mut v = vec![0xFF, 0xD8];
    v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    v.extend_from_slice(&[1u8; 64]);
    v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);
    for class_id in [0x00u8, 0x10] {
        v.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, class_id, 0x01]);
        v.extend_from_slice(&[0u8; 15]);
        v.push(0x00);
    }
    v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    let bits = blocks * 2;
    let mut data = vec![0u8; bits.div_ceil(8)];
    let pad = data.len() * 8 - bits;
    if pad > 0 {
        *data.last_mut().unwrap() |= (1u8 << pad) - 1;
    }
    v.extend_from_slice(&data);
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

#[test]
fn jpeg_is_embedded_unchanged_with_parsed_dimensions() {
    let bytes = gray_jpeg(64, 32);
    let img = Image::jpeg(bytes.clone()).unwrap();
    assert_eq!((img.width(), img.height()), (64, 32));
    let mut d = Document::new();
    d.add_image(img, 128.0);
    // write_and_check runs qpdf --check, which decodes the DCT stream.
    let p = write_and_check("jpeg.pdf", &d);
    let raw = std::fs::read(&p).unwrap();
    assert!(contains(&raw, "/ColorSpace /DeviceGray"));
    assert!(contains(&raw, "/Filter /DCTDecode"));
    assert!(contains(&raw, "/Width 64 /Height 32"));
    assert!(raw.windows(bytes.len()).any(|w| w == &bytes[..]), "jpeg bytes embedded verbatim");
}

#[test]
fn jpeg_header_parsing_and_rejection() {
    // Header-only streams: parse path only, never embedded or decoded.
    let rgb = Image::jpeg(jpeg_header(300, 200, 3)).unwrap();
    assert_eq!((rgb.width(), rgb.height()), (300, 200));
    assert!(matches!(Image::jpeg(jpeg_header(10, 10, 4)), Err(Error::BadJpeg(_))));
    assert!(matches!(Image::jpeg(jpeg_header(0, 10, 3)), Err(Error::BadJpeg(_))));
    // SOF1 with 12-bit samples: /BitsPerComponent 8 would misdescribe it.
    let mut j12 = vec![0xFF, 0xD8, 0xFF, 0xC1, 0x00, 0x0B, 12, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00];
    j12.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFF, 0xD9]);
    assert!(matches!(Image::jpeg(j12), Err(Error::BadJpeg(m)) if m.contains("12")));
}

#[test]
fn explicit_page_breaks_make_pages() {
    let mut d = Document::new();
    d.add_text("page one", Font::Helvetica, 12.0)
        .add_page_break()
        .add_text("page two", Font::Helvetica, 12.0)
        .add_page_break()
        .add_text("page three", Font::Helvetica, 12.0);
    let p = write_and_check("breaks.pdf", &d);
    assert_eq!(page_count(&p), 3);
}

#[test]
fn text_overflow_flows_onto_new_pages() {
    let mut d = Document::new();
    for i in 0..200 {
        d.add_text(&format!("line {i}"), Font::Courier, 12.0);
    }
    let p = write_and_check("flow.pdf", &d);
    assert!(page_count(&p) >= 3);
    assert!(contains(&qdf(&p), "(line 199) Tj"));
}

#[test]
fn mixed_report_multi_page() {
    let mut d = Document::new();
    d.add_text("Annual Report", Font::HelveticaBold, 24.0)
        .add_text(&"Summary text. ".repeat(60), Font::Helvetica, 11.0)
        .add_image(Image::rgb(32, 32, gradient(32, 32)).unwrap(), 200.0)
        .add_page_break()
        .add_table(sample_table(40))
        .add_text("Code: fn main() {}", Font::Courier, 10.0);
    let p = write_and_check("report.pdf", &d);
    assert!(page_count(&p) >= 3);
}

#[test]
fn empty_document_is_still_valid() {
    let p = write_and_check("empty.pdf", &Document::new());
    assert_eq!(page_count(&p), 1);
}

// ---- ISO 32000-2 (PDF 2.0) structural rules this crate relies on ----------
// qpdf --check is not a conformance validator; these assert the specific rules
// the writer implements. Sources: ISO 32000-2:2020 7.5.2 (header), 7.5.4
// (xref), 7.5.5 (trailer), 9.6.2 / Table 109 (Type 1 fonts), as amended by the
// PDF Association errata. NOT covered: content-stream operator validity, page
// tree limits, image dictionary requirements beyond what qpdf --check decodes.

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn reference_doc() -> Vec<u8> {
    let mut d = Document::new();
    d.add_text("a", Font::Helvetica, 12.0)
        .add_image(Image::gray(2, 2, vec![0, 1, 2, 3]).unwrap(), 40.0)
        .add_page_break()
        .add_text("b", Font::CourierBold, 12.0);
    d.render().unwrap()
}

/// (object id -> byte offset) from the xref table that startxref points at.
fn xref_offsets(raw: &[u8]) -> Vec<usize> {
    let sx = rfind(raw, b"startxref\n").expect("startxref");
    let tail = std::str::from_utf8(&raw[sx + 10..]).unwrap();
    let xref_pos: usize = tail.lines().next().unwrap().trim().parse().unwrap();
    assert_eq!(&raw[xref_pos..xref_pos + 5], b"xref\n", "startxref must point at the xref keyword");
    let hdr_end = find(raw, b"\n", xref_pos + 5).unwrap();
    let sub = std::str::from_utf8(&raw[xref_pos + 5..hdr_end]).unwrap();
    let (first, count) = sub.split_once(' ').unwrap();
    assert_eq!(first, "0");
    let count: usize = count.parse().unwrap();
    let mut out = Vec::new();
    let mut p = hdr_end + 1;
    for i in 0..count {
        let e = &raw[p..p + 20];
        assert_eq!(&e[10..11], b" ");
        // 7.5.4: 20-byte entry = 10-digit offset, SP, 5-digit generation, SP, f|n, 2-byte EOL.
        let (gen_want, kind_want): (&[u8], u8) = if i == 0 { (b"65535", b'f') } else { (b"00000", b'n') };
        assert_eq!(&e[11..16], gen_want, "entry {i} generation");
        assert_eq!(e[16], b' ');
        assert_eq!(e[17], kind_want, "entry {i} kind");
        assert_eq!(&e[18..20], b" \n", "entry {i} EOL");
        out.push(std::str::from_utf8(&e[..10]).unwrap().parse().unwrap());
        p += 20;
    }
    out
}

fn object_text(raw: &[u8], offsets: &[usize], id: usize) -> String {
    let start = offsets[id];
    let end = find(raw, b"endobj", start).unwrap();
    String::from_utf8_lossy(&raw[start..end]).into_owned()
}

#[test]
fn header_is_2_0_followed_by_binary_marker_comment() {
    let raw = reference_doc();
    assert!(raw.starts_with(b"%PDF-2.0\n"));
    let line2_end = find(&raw, b"\n", 9).unwrap();
    let line2 = &raw[9..line2_end];
    assert_eq!(line2[0], b'%');
    assert!(line2[1..].len() >= 4 && line2[1..].iter().all(|&b| b >= 128), "needs >= 4 bytes >= 128");
    assert!(raw.ends_with(b"%%EOF\n"));
}

#[test]
fn xref_entries_point_at_their_objects() {
    let raw = reference_doc();
    let offs = xref_offsets(&raw);
    assert!(offs.len() > 10);
    for (id, &o) in offs.iter().enumerate().skip(1) {
        let want = format!("{id} 0 obj\n");
        assert_eq!(&raw[o..o + want.len()], want.as_bytes(), "object {id}");
    }
}

#[test]
fn trailer_has_size_matching_xref_and_root_catalog() {
    let raw = reference_doc();
    let offs = xref_offsets(&raw);
    let t = rfind(&raw, b"trailer\n").unwrap();
    let trailer = String::from_utf8_lossy(&raw[t..]).into_owned();
    assert!(trailer.contains(&format!("/Size {}", offs.len())));
    assert!(trailer.contains("/Root 1 0 R"));
    assert!(object_text(&raw, &offs, 1).contains("/Type /Catalog"));
}

fn numbers_after(s: &str, key: &str) -> Vec<u32> {
    let i = s.find(key).unwrap_or_else(|| panic!("missing {key}")) + key.len();
    let rest = &s[i..];
    let (open, close) = (rest.find('[').unwrap(), rest.find(']').unwrap());
    rest[open + 1..close].split_whitespace().map(|n| n.parse().unwrap()).collect()
}

#[test]
fn every_font_has_descriptor_and_widths_matching_layout_metrics() {
    let raw = reference_doc();
    let offs = xref_offsets(&raw);
    for (i, f) in Font::ALL.iter().enumerate() {
        let font = object_text(&raw, &offs, 3 + i);
        assert!(font.contains("/Subtype /Type1"));
        assert!(font.contains("/Encoding /WinAnsiEncoding"));
        // FontDescriptor is required in PDF 2.0, also for the standard 14 fonts.
        let desc_id = 7 + i;
        assert!(font.contains(&format!("/FontDescriptor {desc_id} 0 R")), "{font}");
        let first: usize = font.split("/FirstChar ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        let last: usize = font.split("/LastChar ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        let widths = numbers_after(&font, "/Widths");
        assert_eq!(widths.len(), last - first + 1, "Widths length must be LastChar-FirstChar+1");
        // Declared widths must equal the widths the layout engine used.
        for code in first..=last {
            let declared = widths[code - first];
            let ch = char::from_u32(code as u32).unwrap();
            if matches!(code, 32..=126 | 160..=255) {
                let used = (f.text_width(&ch.to_string(), 1000.0)).round() as u32;
                assert_eq!(declared, used, "{f:?} code {code}");
                assert!(declared > 0);
            } else {
                assert_eq!(declared, 0, "{f:?} code {code} is never emitted");
            }
        }
        let desc = object_text(&raw, &offs, desc_id);
        for key in ["/Type /FontDescriptor", &format!("/FontName /{}", f.base_name()), "/Flags ", "/FontBBox [", "/ItalicAngle ", "/Ascent ", "/Descent ", "/CapHeight ", "/StemV "] {
            assert!(desc.contains(key), "{f:?} descriptor missing {key}: {desc}");
        }
    }
}

#[test]
fn stream_lengths_match_bytes_between_stream_keywords() {
    let raw = reference_doc();
    let offs = xref_offsets(&raw);
    let mut streams = 0;
    for id in 1..offs.len() {
        let end = find(&raw, b"\nendobj\n", offs[id]).unwrap();
        let obj = &raw[offs[id]..end];
        let Some(s) = find(obj, b"\nstream\n", 0) else { continue };
        let dict = String::from_utf8_lossy(&obj[..s]).into_owned();
        let len: usize = dict.split("/Length ").nth(1).unwrap().split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
        let body = &obj[s + 8..];
        assert!(body.ends_with(b"\nendstream"), "object {id}");
        assert_eq!(body.len() - b"\nendstream".len(), len, "object {id} /Length");
        streams += 1;
    }
    assert!(streams >= 3, "expected content streams + image, saw {streams}");
}

#[test]
fn page_tree_is_consistent() {
    let raw = reference_doc();
    let offs = xref_offsets(&raw);
    let pages = object_text(&raw, &offs, 2);
    assert!(pages.contains("/Count 2"));
    let kids: Vec<usize> = pages.split('[').nth(1).unwrap().split(']').next().unwrap()
        .split_whitespace().filter(|t| *t != "0" && *t != "R").map(|t| t.parse().unwrap()).collect();
    assert_eq!(kids.len(), 2);
    for k in kids {
        let p = object_text(&raw, &offs, k);
        assert!(p.contains("/Type /Page"));
        assert!(p.contains("/Parent 2 0 R"));
        assert!(p.contains("/MediaBox [0 0 595 842]"));
    }
}

/// Builds a classic-xref PDF from object bodies (ids 1..) and a trailer dict body.
fn raw_pdf(header: &str, objs: &[&str], trailer: &str) -> Vec<u8> {
    let mut b = format!("{header}\n%\u{e2}\u{e3}\n").into_bytes();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(b.len());
        b.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = b.len();
    b.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        b.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    b.extend_from_slice(format!("trailer\n<< /Size {} {trailer} >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    b
}

const ID: &str = "/ID [<00112233445566778899AABBCCDDEEFF> <00112233445566778899AABBCCDDEEFF>]";
const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> >>";

/// The checker must accept a minimal conforming file, otherwise every
/// negative case below would pass for the wrong reason.
#[test]
fn arlington_accepts_minimal_valid_file() {
    let pdf = raw_pdf("%PDF-2.0", &["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", PAGE], &format!("/Root 1 0 R {ID}"));
    assert_eq!(common::arlington_errors(&pdf), Vec::<String>::new());
}

#[test]
fn arlington_rejects_known_violations() {
    let cat = "<< /Type /Catalog /Pages 2 0 R >>";
    let pages = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    let font = |extra: &str| format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Font << /F1 4 0 R >> >> >>{extra}");
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("missing /ID", raw_pdf("%PDF-2.0", &[cat, pages, PAGE], "/Root 1 0 R"), "trailer/ID: required"),
        ("short /ID", raw_pdf("%PDF-2.0", &[cat, pages, PAGE], "/Root 1 0 R /ID [<00> <00>]"), "StringLength(0)>=16"),
        ("1.7 header", raw_pdf("%PDF-1.7", &[cat, pages, PAGE], &format!("/Root 1 0 R {ID}")), "not %PDF-2.0"),
        ("wrong catalog /Type", raw_pdf("%PDF-2.0", &["<< /Type /Katalog /Pages 2 0 R >>", pages, PAGE], &format!("/Root 1 0 R {ID}")), "not one of [Catalog]"),
        ("undefined key", raw_pdf("%PDF-2.0", &["<< /Type /Catalog /Pages 2 0 R /Bogus 1 >>", pages, PAGE], &format!("/Root 1 0 R {ID}")), "Bogus: key not defined in Catalog"),
        ("missing MediaBox", raw_pdf("%PDF-2.0", &[cat, pages, "<< /Type /Page /Parent 2 0 R /Resources << >> >>"], &format!("/Root 1 0 R {ID}")), "MediaBox: required"),
        ("inherited MediaBox is fine", raw_pdf("%PDF-2.0", &[cat, "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 1 1] >>", "<< /Type /Page /Parent 2 0 R /Resources << >> >>"], &format!("/Root 1 0 R {ID}")), ""),
        ("direct Pages", raw_pdf("%PDF-2.0", &["<< /Type /Catalog /Pages << /Type /Pages /Kids [] /Count 0 >> >>"], &format!("/Root 1 0 R {ID}")), "Pages: must be an indirect reference"),
        ("bad Rotate", raw_pdf("%PDF-2.0", &[cat, pages, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Rotate 45 >>"], &format!("/Root 1 0 R {ID}")), "Rotate mod 90"),
        ("non-rectangle MediaBox", raw_pdf("%PDF-2.0", &[cat, pages, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10] /Resources << >> >>"], &format!("/Root 1 0 R {ID}")), "is not of type `rectangle`"),
        ("font without Widths in 2.0", raw_pdf("%PDF-2.0", &[cat, pages, &font(""), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"], &format!("/Root 1 0 R {ID}")), "Widths: required"),
        ("Widths length mismatch", raw_pdf("%PDF-2.0", &[cat, pages, &font(""), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 33 /Widths [1] /FontDescriptor 5 0 R >>", "<< /Type /FontDescriptor /FontName /Helvetica /Flags 32 /FontBBox [0 0 1 1] /ItalicAngle 0 /Ascent 1 /Descent 0 /StemV 1 >>"], &format!("/Root 1 0 R {ID}")), "ArrayLength(Widths)"),
        ("FontName differs from BaseFont", raw_pdf("%PDF-2.0", &[cat, pages, &font(""), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 32 /Widths [1] /FontDescriptor 5 0 R >>", "<< /Type /FontDescriptor /FontName /Courier /Flags 32 /FontBBox [0 0 1 1] /ItalicAngle 0 /Ascent 1 /Descent 0 /StemV 1 >>"], &format!("/Root 1 0 R {ID}")), "FontName==parent::@BaseFont"),
        ("reserved Flags bit", raw_pdf("%PDF-2.0", &[cat, pages, &font(""), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 32 /Widths [1] /FontDescriptor 5 0 R >>", "<< /Type /FontDescriptor /FontName /Helvetica /Flags 288 /FontBBox [0 0 1 1] /ItalicAngle 0 /Ascent 1 /Descent 0 /StemV 1 >>"], &format!("/Root 1 0 R {ID}")), "BitsClear(8,16)"),
        ("positive Descent", raw_pdf("%PDF-2.0", &[cat, pages, &font(""), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 32 /Widths [1] /FontDescriptor 5 0 R >>", "<< /Type /FontDescriptor /FontName /Helvetica /Flags 32 /FontBBox [0 0 1 1] /ItalicAngle 0 /Ascent 1 /Descent 5 /StemV 1 >>"], &format!("/Root 1 0 R {ID}")), "Descent<=0"),
        ("DCT image with 4 bits", raw_pdf("%PDF-2.0", &[cat, pages, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Im1 4 0 R >> >> >>", "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 4 /Filter /DCTDecode /Length 1 >>\nstream\nx\nendstream"], &format!("/Root 1 0 R {ID}")), "must be 8"),
        ("unreachable object", raw_pdf("%PDF-2.0", &[cat, pages, PAGE, "<< /Orphan true >>"], &format!("/Root 1 0 R {ID}")), "object 4 0 R is not reachable"),
    ];
    for (what, pdf, want) in cases {
        let errs = common::arlington_errors(&pdf);
        if want.is_empty() {
            assert!(errs.is_empty(), "{what}: expected no errors, got {errs:#?}");
        } else {
            assert!(errs.iter().any(|e| e.contains(want)), "{what}: expected an error containing {want:?}, got {errs:#?}");
        }
    }
}

/// One page whose content stream is `content` (uncompressed), with font /F1
/// (Helvetica, codes 32..126) and image /Im1 in its resources.
fn page_with_content(content: &str) -> Vec<u8> {
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> >>";
    let stream = format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1);
    raw_pdf(
        "%PDF-2.0",
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            page,
            &stream,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 126 >>",
            "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nx\nendstream",
        ],
        &format!("/Root 1 0 R {ID}"),
    )
}

#[test]
fn content_checker_accepts_valid_and_rejects_known_violations() {
    let ok = "q 1 0 0 1 0 0 cm /Im1 Do Q BT /F1 12 Tf 1 2 Td (Hi) Tj [(a) -50 (b)] TJ ET 0.5 w 0 0 1 1 re S 0 0 m 1 1 l h f /Span BMC EMC";
    assert_eq!(common::content::content_errors(&page_with_content(ok)), Vec::<String>::new());
    let cases = [
        ("BT (x) Tj ET", "before Tf"),
        ("BT /F1 12 Tf ET", ""),
        ("BT /F9 12 Tf ET", "/F9 is not in Resources /Font"),
        ("/Im9 Do", "/Im9 is not in Resources /XObject"),
        ("BT /F1 12 Tf (\\351) Tj ET", "byte 233 outside font FirstChar..LastChar 32..126"),
        ("BT /F1 12 Tf 1 Td ET", "operands"),
        ("BT /F1 Tf ET", "operands"),
        ("(x) Tj", "not allowed in Page state"),
        ("BT q Q ET", "not allowed in Text state"),
        ("BT /F1 12 Tf (x) Tj", "ends in Text state"),
        ("0 0 m 1 1 l", "ends in Path state"),
        ("0 0 m BT ET", "not allowed in Path state"),
        ("f", "not allowed in Page state"),
        ("q", "q without matching Q"),
        ("Q", "Q without matching q"),
        ("EMC", "EMC without BMC/BDC"),
        ("/Span BMC", "BMC/BDC without EMC"),
        ("/Span /P1 BDC EMC", "/P1 is not in Resources /Properties"),
        ("/CS0 cs", "/CS0 is not in Resources /ColorSpace"),
        ("/DeviceRGB cs 1 0 0 sc", ""),
        ("1 2 3 4 5 sc", "operands"),
        ("0 0 Zz", "not a PDF 2.0 content operator"),
    ];
    for (src, want) in cases {
        let errs = common::content::content_errors(&page_with_content(src));
        if want.is_empty() {
            assert!(errs.is_empty(), "`{src}`: expected no errors, got {errs:#?}");
        } else {
            assert!(errs.iter().any(|e| e.contains(want)), "`{src}`: expected an error containing {want:?}, got {errs:#?}");
        }
    }
}
