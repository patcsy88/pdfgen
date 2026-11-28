//! Minimal PDF report generation: wrapped text, standard fonts, tables,
//! images (raw RGB/gray and JPEG) and explicit page breaks.
//!
//! Fonts are the PDF base-14 faces Helvetica, Helvetica-Bold, Courier and
//! Courier-Bold with WinAnsi encoding (ASCII and Latin-1; other characters
//! render as `?`). No font files are embedded. Widths and descriptor metrics
//! come from the Adobe core-14 AFM files vendored in `core/afm/`.

mod fontdata;

use fontdata::Metrics;
use std::fmt;

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 50.0;
const CELL_PAD: f32 = 4.0;
const LEADING: f32 = 1.2;
/// Largest font size `render` accepts. A line is `size * LEADING` tall, so this keeps every coordinate
/// written to a content stream well inside the +-32767 limit that PDF consumers implement (ISO 32000-2 Annex C).
const MAX_FONT_SIZE: f32 = 1000.0;
/// Largest text size `render` accepts: one line (`size * LEADING`) exactly fills the text area of a page.
/// Table font sizes keep [`MAX_FONT_SIZE`]; a row too tall for a page fails with [`Error::RowTooTall`].
const MAX_TEXT_SIZE: f32 = (PAGE_H - 2.0 * MARGIN) / LEADING;
/// Largest x coordinate `render` lets a table reach.
const MAX_COORD: f32 = 32767.0;
/// Smallest text or table font size `render` accepts. Without a lower bound a tiny size puts a whole input
/// on one line, and the per-line work (and the page count a tall line needs) is no longer bounded by the input.
const MIN_FONT_SIZE: f32 = 1.0;
/// Most pages `render` lays out before failing with [`Error::TooLarge`].
const MAX_PAGES: usize = 10_000;
/// Most uncompressed page-content bytes `render` lays out before failing with [`Error::TooLarge`].
const MAX_CONTENT_BYTES: usize = 256 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Raw pixel buffer length does not match width * height * channels.
    BadImageData(String),
    /// JPEG stream could not be parsed or uses an unsupported colour model.
    BadJpeg(String),
    /// Table has no columns, or a row has more cells than columns.
    BadTable(String),
    /// A table row is taller than a full page, or taller than a page together with the table header.
    RowTooTall,
    /// A font size, column width or image width is NaN, infinite, not positive, or above its limit, or a
    /// column width leaves no room inside its cell padding.
    BadDimension(String),
    /// The document would exceed `MAX_PAGES` (10 000) pages or `MAX_CONTENT_BYTES` (256 MiB) bytes of
    /// uncompressed page content and image data.
    TooLarge(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadImageData(m) => write!(f, "bad image data: {m}"),
            Error::BadJpeg(m) => write!(f, "bad jpeg: {m}"),
            Error::BadTable(m) => write!(f, "bad table: {m}"),
            Error::RowTooTall => write!(f, "table row taller than a page (together with the header)"),
            Error::BadDimension(m) => write!(f, "bad dimension: {m}"),
            Error::TooLarge(m) => write!(f, "document too large: {m}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Helvetica,
    HelveticaBold,
    Courier,
    CourierBold,
}

impl Font {
    pub const ALL: [Font; 4] = [
        Font::Helvetica,
        Font::HelveticaBold,
        Font::Courier,
        Font::CourierBold,
    ];

    fn index(self) -> usize {
        self as usize
    }

    pub fn base_name(self) -> &'static str {
        match self {
            Font::Helvetica => "Helvetica",
            Font::HelveticaBold => "Helvetica-Bold",
            Font::Courier => "Courier",
            Font::CourierBold => "Courier-Bold",
        }
    }

    fn metrics(self) -> &'static Metrics {
        match self {
            Font::Helvetica => &fontdata::HELVETICA,
            Font::HelveticaBold => &fontdata::HELVETICA_BOLD,
            Font::Courier => &fontdata::COURIER,
            Font::CourierBold => &fontdata::COURIER_BOLD,
        }
    }

    /// Advance width in 1/1000 em of the WinAnsi code that `c` is written as.
    fn char_width(self, c: char) -> u32 {
        self.metrics().widths[winansi_code(c) as usize] as u32
    }

    pub fn text_width(self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.char_width(c) as u64).sum::<u64>() as f32 * size / 1000.0
    }
}

/// WinAnsi byte for `c`: printable ASCII and Latin-1 pass through, anything
/// else is written (and measured) as `?`.
fn winansi_code(c: char) -> u8 {
    match c as u32 {
        32..=126 | 160..=255 => c as u32 as u8,
        _ => b'?',
    }
}

/// Pixel data for an image.
#[derive(Debug, Clone)]
pub struct Image {
    width: u32,
    height: u32,
    data: ImageData,
}

#[derive(Debug, Clone)]
enum ImageData {
    Rgb(Vec<u8>),
    Gray(Vec<u8>),
    Jpeg { bytes: Vec<u8>, components: u8 },
}

impl Image {
    /// 8-bit RGB, row-major, `width * height * 3` bytes.
    pub fn rgb(width: u32, height: u32, data: Vec<u8>) -> Result<Image, Error> {
        check_len(width, height, 3, data.len())?;
        Ok(Image { width, height, data: ImageData::Rgb(data) })
    }

    /// 8-bit grayscale, row-major, `width * height` bytes.
    pub fn gray(width: u32, height: u32, data: Vec<u8>) -> Result<Image, Error> {
        check_len(width, height, 1, data.len())?;
        Ok(Image { width, height, data: ImageData::Gray(data) })
    }

    /// 8-bit baseline/progressive JPEG, embedded unchanged. Gray and RGB only.
    pub fn jpeg(bytes: Vec<u8>) -> Result<Image, Error> {
        let (width, height, components) = parse_jpeg(&bytes)?;
        if components != 1 && components != 3 {
            return Err(Error::BadJpeg(format!("{components} components unsupported")));
        }
        Ok(Image { width, height, data: ImageData::Jpeg { bytes, components } })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

fn check_len(w: u32, h: u32, ch: usize, len: usize) -> Result<(), Error> {
    let want = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(ch))
        .ok_or_else(|| Error::BadImageData("dimensions overflow".into()))?;
    if w == 0 || h == 0 {
        return Err(Error::BadImageData("zero dimension".into()));
    }
    if want != len {
        return Err(Error::BadImageData(format!("expected {want} bytes, got {len}")));
    }
    Ok(())
}

/// Largest frame a JPEG header may declare. The stream is embedded unchanged, so the declared size is what a
/// viewer allocates for; a 16-bit header alone can claim 65535 x 65535.
const MAX_JPEG_PIXELS: u64 = 100_000_000;

fn parse_jpeg(b: &[u8]) -> Result<(u32, u32, u8), Error> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return Err(Error::BadJpeg("missing SOI marker".into()));
    }
    let mut i = 2;
    let mut frame = None;
    let mut sos = false;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return Err(Error::BadJpeg("expected marker".into()));
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        // EOI before SOS: nothing after it is image data, so a frame header past it would be a decoy.
        if m == 0xD9 {
            break;
        }
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        // The length counts its own two bytes. A smaller value would make the scan below re-read the segment's
        // own bytes as markers, which a real decoder does not.
        if len < 2 {
            return Err(Error::BadJpeg("segment length below 2".into()));
        }
        if i + 2 + len > b.len() {
            return Err(Error::BadJpeg("truncated segment".into()));
        }
        // SOF0-SOF15 minus DHT (C4), JPG (C8) and DAC (CC). A decoder uses one frame header, so a second one,
        // or one for a coding process this code does not describe, would let a decoy frame pass the checks below.
        if matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            if frame.is_some() {
                return Err(Error::BadJpeg("more than one frame header".into()));
            }
            if m > 0xC2 {
                return Err(Error::BadJpeg(format!(
                    "SOF marker 0x{m:02X} unsupported, only baseline and progressive Huffman (SOF0-SOF2)"
                )));
            }
            if len < 8 {
                return Err(Error::BadJpeg("truncated SOF".into()));
            }
            let p = b[i + 4];
            if p != 8 {
                return Err(Error::BadJpeg(format!("{p}-bit samples unsupported, only 8-bit")));
            }
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            let n = b[i + 9];
            if len != 8 + 3 * n as usize {
                return Err(Error::BadJpeg(format!("SOF length {len} does not match {n} components")));
            }
            if w == 0 || h == 0 {
                return Err(Error::BadJpeg("zero dimension".into()));
            }
            if w as u64 * h as u64 > MAX_JPEG_PIXELS {
                return Err(Error::BadJpeg(format!(
                    "{w}x{h} exceeds the {MAX_JPEG_PIXELS} pixel limit"
                )));
            }
            frame = Some((w, h, n));
        } else if m == 0xDA {
            sos = true;
            break;
        }
        i += 2 + len;
    }
    let frame = frame.ok_or_else(|| Error::BadJpeg("no SOF0-SOF2 marker found".into()))?;
    if !sos {
        return Err(Error::BadJpeg("no SOS marker before end of image".into()));
    }
    Ok(frame)
}

#[derive(Debug, Clone)]
pub struct Table {
    /// Column widths in points. Cells beyond this many columns are an error.
    pub column_widths: Vec<f32>,
    pub header: Option<Vec<String>>,
    pub rows: Vec<Vec<String>>,
    pub font_size: f32,
}

#[derive(Debug, Clone)]
pub enum Block {
    Text { text: String, font: Font, size: f32 },
    Table(Table),
    /// `width` in points; height follows the aspect ratio.
    Image { image: Image, width: f32 },
    PageBreak,
}

#[derive(Debug, Clone, Default)]
pub struct Document {
    blocks: Vec<Block>,
}

impl Document {
    pub fn new() -> Document {
        Document::default()
    }

    pub fn add(&mut self, block: Block) -> &mut Self {
        self.blocks.push(block);
        self
    }

    pub fn add_text(&mut self, text: &str, font: Font, size: f32) -> &mut Self {
        self.add(Block::Text { text: text.to_string(), font, size })
    }

    pub fn add_table(&mut self, table: Table) -> &mut Self {
        self.add(Block::Table(table))
    }

    pub fn add_image(&mut self, image: Image, width: f32) -> &mut Self {
        self.add(Block::Image { image, width })
    }

    pub fn add_page_break(&mut self) -> &mut Self {
        self.add(Block::PageBreak)
    }

    /// Lay out all blocks and serialise a PDF 2.0 file.
    ///
    /// Fails with [`Error::BadDimension`] before any layout if a text size is not in `[1, 618.33]` (one line
    /// as tall as the page's text area), a table font size is not in `[1, 1000]`, a column width is not
    /// finite and above 8 (twice the cell padding), the columns together reach beyond x = 32767,
    /// or an image width is not positive and finite. An image wider than the text area is scaled down.
    ///
    /// Fails with [`Error::RowTooTall`] if a table row, together with the table's header if it has one,
    /// is taller than a page.
    ///
    /// Fails with [`Error::BadDimension`] during layout if an image would render with a side that
    /// underflows to zero.
    ///
    /// Fails with [`Error::TooLarge`] if the layout needs more than 10 000 pages or more than 256 MiB of
    /// uncompressed page content and image data; layout stops as soon as a limit is passed.
    pub fn render(&self) -> Result<Vec<u8>, Error> {
        for b in &self.blocks {
            match b {
                Block::Text { size, .. } => check_dim("text size", *size, MIN_FONT_SIZE, MAX_TEXT_SIZE)?,
                Block::Table(t) => check_table_dims(t)?,
                Block::Image { width, .. } => check_dim("image width", *width, 0.0, f32::MAX)?,
                Block::PageBreak => {}
            }
        }
        let mut lay = Layout::new();
        for b in &self.blocks {
            match b {
                Block::Text { text, font, size } => lay.text(text, *font, *size)?,
                Block::Table(t) => lay.table(t)?,
                Block::Image { image, width } => lay.image(image, *width)?,
                Block::PageBreak => lay.new_page()?,
            }
        }
        Ok(lay.finish())
    }
}

/// `v` must be finite, positive, at least `min` and at most `max`.
fn check_dim(what: &str, v: f32, min: f32, max: f32) -> Result<(), Error> {
    if v.is_finite() && v > 0.0 && v >= min && v <= max {
        Ok(())
    } else if min > 0.0 {
        Err(Error::BadDimension(format!("{what} must be at least {min} and at most {max}, got {v}")))
    } else {
        Err(Error::BadDimension(format!("{what} must be greater than 0 and at most {max}, got {v}")))
    }
}

fn check_table_dims(t: &Table) -> Result<(), Error> {
    check_dim("table font size", t.font_size, MIN_FONT_SIZE, MAX_FONT_SIZE)?;
    for w in &t.column_widths {
        check_dim("column width", *w, 0.0, MAX_COORD)?;
        if *w <= 2.0 * CELL_PAD {
            return Err(Error::BadDimension(format!("column width must be above {}, got {w}", 2.0 * CELL_PAD)));
        }
    }
    let total: f32 = t.column_widths.iter().sum();
    if !(total <= MAX_COORD - MARGIN) {
        return Err(Error::BadDimension(format!("column widths add up to {total}, more than {}", MAX_COORD - MARGIN)));
    }
    Ok(())
}

/// Wraps each cell of `row` into its column and returns the lines plus the row height.
///
/// Wrapping stops at the first cell that already needs more than a page: the row is rejected as
/// [`Error::RowTooTall`] whatever its other cells hold, so they are neither wrapped nor allocated.
fn layout_row(row: &[String], is_header: bool, t: &Table) -> (Vec<Vec<String>>, f32) {
    let font = if is_header { Font::HelveticaBold } else { Font::Helvetica };
    let size = t.font_size;
    // One line more than fits a page: enough lines to be certain the row is too tall.
    let max_lines = ((PAGE_H - 2.0 * MARGIN) / (size * LEADING)) as usize + 2;
    let mut cells = Vec::with_capacity(t.column_widths.len());
    let mut lines = 1;
    for (i, w) in t.column_widths.iter().enumerate() {
        let cell = wrap(row.get(i).map(String::as_str).unwrap_or(""), font, size, w - 2.0 * CELL_PAD, max_lines);
        let n = cell.len();
        lines = lines.max(n);
        cells.push(cell);
        if n >= max_lines {
            break;
        }
    }
    (cells, lines as f32 * size * LEADING + 2.0 * CELL_PAD)
}

struct PageBuf {
    content: Vec<u8>,
    images: Vec<usize>,
}

struct Layout<'a> {
    pages: Vec<PageBuf>,
    /// Content bytes of every page but the last.
    done_bytes: usize,
    y: f32,
    images: Vec<&'a Image>,
    /// Bytes of image data handed to `finish`; counted against `MAX_CONTENT_BYTES` with the page content.
    image_bytes: usize,
}

impl<'a> Layout<'a> {
    fn new() -> Self {
        Layout {
            pages: vec![PageBuf { content: Vec::new(), images: Vec::new() }],
            done_bytes: 0,
            y: PAGE_H - MARGIN,
            images: Vec::new(),
            image_bytes: 0,
        }
    }

    fn at_top(&self) -> bool {
        self.y >= PAGE_H - MARGIN - 0.01
    }

    fn new_page(&mut self) -> Result<(), Error> {
        if self.pages.len() >= MAX_PAGES {
            return Err(Error::TooLarge(format!("more than {MAX_PAGES} pages")));
        }
        self.done_bytes += self.cur().content.len();
        self.pages.push(PageBuf { content: Vec::new(), images: Vec::new() });
        self.y = PAGE_H - MARGIN;
        Ok(())
    }

    /// Fails once the page content and image data written so far pass `MAX_CONTENT_BYTES`.
    fn check_size(&self) -> Result<(), Error> {
        let total = self.done_bytes + self.pages.last().map_or(0, |p| p.content.len()) + self.image_bytes;
        if total > MAX_CONTENT_BYTES {
            return Err(Error::TooLarge(format!("page content and image data exceed {MAX_CONTENT_BYTES} bytes")));
        }
        Ok(())
    }

    fn cur(&mut self) -> &mut PageBuf {
        self.pages.last_mut().unwrap()
    }

    fn put_text(&mut self, x: f32, y: f32, font: Font, size: f32, s: &str) {
        let c = &mut self.cur().content;
        c.extend_from_slice(
            format!("BT /F{} {} Tf {:.2} {:.2} Td (", font.index() + 1, size, x, y).as_bytes(),
        );
        for ch in s.chars() {
            let b = winansi_code(ch);
            match b {
                b'(' | b')' | b'\\' => {
                    c.push(b'\\');
                    c.push(b);
                }
                0x80..=0xFF => c.extend_from_slice(format!("\\{:03o}", b).as_bytes()),
                _ => c.push(b),
            }
        }
        c.extend_from_slice(b") Tj ET\n");
    }

    fn text(&mut self, text: &str, font: Font, size: f32) -> Result<(), Error> {
        let lh = size * LEADING;
        // More lines than the remaining pages can hold make `new_page` fail with `TooLarge`, so wrapping
        // further would only allocate lines that are never placed.
        let per_page = ((PAGE_H - 2.0 * MARGIN) / lh) as usize + 1;
        let max_lines = (MAX_PAGES - self.pages.len() + 1) * per_page + 1;
        for line in wrap(text, font, size, PAGE_W - 2.0 * MARGIN, max_lines) {
            if self.y - lh < MARGIN && !self.at_top() {
                self.new_page()?;
            }
            self.y -= lh;
            let y = self.y + size * 0.2;
            self.put_text(MARGIN, y, font, size, &line);
            self.check_size()?;
        }
        self.y -= size * 0.4; // paragraph gap
        Ok(())
    }

    fn table(&mut self, t: &Table) -> Result<(), Error> {
        let ncols = t.column_widths.len();
        if ncols == 0 {
            return Err(Error::BadTable("no columns".into()));
        }
        let too_wide = |r: &Vec<String>| r.len() > ncols;
        if t.header.iter().any(too_wide) || t.rows.iter().any(too_wide) {
            return Err(Error::BadTable("row has more cells than columns".into()));
        }
        let size = t.font_size;
        let max_row_h = PAGE_H - 2.0 * MARGIN;

        let header = t.header.as_ref().map(|h| layout_row(h, true, t));
        let header_h = header.as_ref().map_or(0.0, |(_, h)| *h);
        if header_h > max_row_h {
            return Err(Error::RowTooTall);
        }
        let rows = t.rows.iter().map(|r| layout_row(r, false, t));
        if let Some((cells, h)) = &header {
            if self.y - h < MARGIN && !self.at_top() {
                self.new_page()?;
            }
            self.draw_row(cells, Font::HelveticaBold, size, *h, &t.column_widths, true)?;
        }
        for (cells, h) in rows {
            // A row that cannot fit under the header repeated on a fresh page can never be placed.
            if header_h + h > max_row_h {
                return Err(Error::RowTooTall);
            }
            if self.y - h < MARGIN && !self.at_top() {
                self.new_page()?;
                if let Some((hc, hh)) = &header {
                    self.draw_row(hc, Font::HelveticaBold, size, *hh, &t.column_widths, true)?;
                }
            }
            self.draw_row(&cells, Font::Helvetica, size, h, &t.column_widths, false)?;
        }
        self.y -= size * 0.6;
        Ok(())
    }

    fn draw_row(&mut self, cells: &[Vec<String>], font: Font, size: f32, h: f32, widths: &[f32], shade: bool) -> Result<(), Error> {
        let top = self.y;
        let lh = size * LEADING;
        let mut x = MARGIN;
        for (i, lines) in cells.iter().enumerate() {
            let w = widths[i];
            let c = &mut self.cur().content;
            if shade {
                c.extend_from_slice(format!("0.9 g {:.2} {:.2} {:.2} {:.2} re f 0 g\n", x, top - h, w, h).as_bytes());
            }
            c.extend_from_slice(format!("0.5 w {:.2} {:.2} {:.2} {:.2} re S\n", x, top - h, w, h).as_bytes());
            for (n, line) in lines.iter().enumerate() {
                let y = top - CELL_PAD - (n as f32 + 1.0) * lh + size * 0.2 + (lh - size) / 2.0;
                self.put_text(x + CELL_PAD, y, font, size, line);
            }
            x += w;
            self.check_size()?;
        }
        self.y = top - h;
        Ok(())
    }

    fn image(&mut self, img: &'a Image, width: f32) -> Result<(), Error> {
        let max_h = PAGE_H - 2.0 * MARGIN;
        let max_w = PAGE_W - 2.0 * MARGIN;
        let mut w = width.min(max_w);
        let mut h = w * img.height as f32 / img.width as f32;
        if h > max_h {
            h = max_h;
            w = h * img.width as f32 / img.height as f32;
        }
        if !(w > 0.0 && h > 0.0) {
            return Err(Error::BadDimension(format!("image renders with a zero-sized side ({w} x {h})")));
        }
        if self.y - h < MARGIN && !self.at_top() {
            self.new_page()?;
        }
        // Every block owns its own `Image`, so a pointer-identity lookup never hits: one XObject per block.
        self.images.push(img);
        self.image_bytes += match &img.data {
            ImageData::Rgb(d) | ImageData::Gray(d) => d.len(),
            ImageData::Jpeg { bytes, .. } => bytes.len(),
        };
        let idx = self.images.len() - 1;
        let y = self.y - h;
        let page = self.cur();
        page.images.push(idx);
        // Two decimals, except for a side under 0.01 pt: `{:.2}` would print 0.00, a singular matrix.
        let side = |v: f32| if v >= 0.01 { format!("{v:.2}") } else { format!("{v}") };
        page.content.extend_from_slice(
            format!("q {} 0 0 {} {:.2} {:.2} cm /Im{} Do Q\n", side(w), side(h), MARGIN, y, idx + 1).as_bytes(),
        );
        self.y = y - 8.0;
        self.check_size()
    }

    fn finish(self) -> Vec<u8> {
        // Object ids: 1 catalog, 2 pages, 3..=6 fonts, 7..=10 font descriptors,
        // then images, then (content, page) pairs.
        let first_image = 11usize;
        let first_page = first_image + self.images.len();
        let mut w = Writer::new();

        w.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: String = (0..self.pages.len()).map(|i| format!("{} 0 R ", first_page + 2 * i + 1)).collect();
        w.obj(2, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids, self.pages.len()).into_bytes());
        for f in Font::ALL {
            let m = f.metrics();
            // ISO 32000-2 Table 109: FontDescriptor is required, also for the
            // standard 14 fonts. FirstChar/LastChar/Widths cover WinAnsi 32..=255;
            // codes this crate never emits (127..=159) have width 0.
            let widths: String = m.widths[32..=255].iter().map(|w| format!("{w} ")).collect();
            w.obj(
                3 + f.index(),
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 255 /Widths [{}] /FontDescriptor {} 0 R >>",
                    f.base_name(),
                    widths,
                    7 + f.index()
                )
                .into_bytes(),
            );
            let flags = if matches!(f, Font::Courier | Font::CourierBold) { 33 } else { 32 }; // FixedPitch? + Nonsymbolic
            w.obj(
                7 + f.index(),
                format!(
                    "<< /Type /FontDescriptor /FontName /{} /Flags {} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV {} /MissingWidth 0 >>",
                    f.base_name(), flags, m.bbox[0], m.bbox[1], m.bbox[2], m.bbox[3],
                    m.italic_angle, m.ascent, m.descent, m.cap_height, m.stem_v
                )
                .into_bytes(),
            );
        }
        for (i, img) in self.images.iter().enumerate() {
            let (cs, filter, data) = match &img.data {
                ImageData::Rgb(d) => ("DeviceRGB", "FlateDecode", deflate(d)),
                ImageData::Gray(d) => ("DeviceGray", "FlateDecode", deflate(d)),
                ImageData::Jpeg { bytes, components } => (
                    if *components == 1 { "DeviceGray" } else { "DeviceRGB" },
                    "DCTDecode",
                    bytes.clone(),
                ),
            };
            let dict = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{} /BitsPerComponent 8 /Filter /{} /Length {} >>",
                img.width, img.height, cs, filter, data.len()
            );
            w.stream(first_image + i, dict, data);
        }
        let fonts: String = Font::ALL.iter().map(|f| format!("/F{} {} 0 R ", f.index() + 1, 3 + f.index())).collect();
        for (i, p) in self.pages.iter().enumerate() {
            let cid = first_page + 2 * i;
            let data = deflate(&p.content);
            w.stream(cid, format!("<< /Filter /FlateDecode /Length {} >>", data.len()), data);
            let xo: String = p.images.iter().map(|&k| format!("/Im{} {} 0 R ", k + 1, first_image + k)).collect();
            w.obj(
                cid + 1,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {} 0 R /Resources << /Font << {}>> /XObject << {}>> >> >>",
                    PAGE_W, PAGE_H, cid, fonts, xo
                )
                .into_bytes(),
            );
        }
        w.finish()
    }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// Greedy word wrap; hard-breaks words wider than `max_w`. Returns between 1 and `max_lines` lines
/// (`max_lines` must be at least 1). Once that many lines exist the rest of `text` is not wrapped, so the
/// allocation is bounded by the caller's limit and not by the input size.
///
/// Widths are tracked as running sums of per-character advances, so the cost is linear in `text`.
fn wrap(text: &str, font: Font, size: f32, max_w: f32, max_lines: usize) -> Vec<String> {
    let fits = |units: u64| units as f32 * size / 1000.0 <= max_w;
    let space = font.char_width(' ') as u64;
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut line_units = 0u64;
        for word in para.split(|c: char| c.is_whitespace() && c != '\u{a0}').filter(|w| !w.is_empty()) {
            let word_units: u64 = word.chars().map(|c| font.char_width(c) as u64).sum();
            let cand_units = if line.is_empty() { word_units } else { line_units + space + word_units };
            if fits(cand_units) {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
                line_units = cand_units;
                continue;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
                if out.len() >= max_lines {
                    return out;
                }
            }
            let mut cur = String::new();
            let mut cur_units = 0u64;
            for ch in word.chars() {
                let cw = font.char_width(ch) as u64;
                if !cur.is_empty() && !fits(cur_units + cw) {
                    out.push(std::mem::take(&mut cur));
                    if out.len() >= max_lines {
                        return out;
                    }
                    cur_units = 0;
                }
                cur.push(ch);
                cur_units += cw;
            }
            line = cur;
            line_units = cur_units;
        }
        out.push(line);
        if out.len() >= max_lines {
            return out;
        }
    }
    out
}

struct Writer {
    buf: Vec<u8>,
    offsets: Vec<(usize, usize)>, // (id, offset)
}

impl Writer {
    fn new() -> Self {
        Writer { buf: b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: Vec::new() }
    }

    fn obj(&mut self, id: usize, body: Vec<u8>) {
        self.offsets.push((id, self.buf.len()));
        self.buf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        self.buf.extend_from_slice(&body);
        self.buf.extend_from_slice(b"\nendobj\n");
    }

    fn stream(&mut self, id: usize, dict: String, data: Vec<u8>) {
        let mut body = dict.into_bytes();
        body.extend_from_slice(b"\nstream\n");
        body.extend_from_slice(&data);
        body.extend_from_slice(b"\nendstream");
        self.obj(id, body);
    }

    fn finish(mut self) -> Vec<u8> {
        self.offsets.sort_unstable();
        let n = self.offsets.last().map(|o| o.0).unwrap_or(0) + 1;
        let xref = self.buf.len();
        self.buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
        for (id, off) in &self.offsets {
            debug_assert_eq!(*id, self.offsets.iter().position(|o| o.0 == *id).unwrap() + 1);
            self.buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        // ISO 32000-2 14.4: /ID is required in PDF 2.0, two byte strings of at least
        // 16 bytes. Derived from the body so output stays deterministic; both halves
        // are equal because every file is a new, unmodified document.
        let id: String = [0xcbf2_9ce4_8422_2325u64, 0x6c62_272e_07bb_0142]
            .iter()
            .map(|&seed| format!("{:016X}", fnv1a64(seed, &self.buf)))
            .collect();
        self.buf.extend_from_slice(
            format!("trailer\n<< /Size {n} /Root 1 0 R /ID [<{id}> <{id}>] >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        self.buf
    }
}

fn fnv1a64(seed: u64, data: &[u8]) -> u64 {
    data.iter().fold(seed, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3))
}
