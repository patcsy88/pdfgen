//! C ABI over `pdfgen-core`.
//!
//! Conventions
//! - Handles are opaque pointers; free each with its matching `*_free`.
//! - Functions returning `i32` return 0 on success, a negative code on failure;
//!   the message is available from `pdfgen_last_error` (thread-local, valid until
//!   the next pdfgen call on the same thread).
//! - Strings are NUL-terminated UTF-8.
//! - Panics never cross the boundary; they become `PDFGEN_ERR_PANIC`.

use pdfgen_core::{Block, Document, Font, Image, Table};
use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

pub const PDFGEN_OK: i32 = 0;
pub const PDFGEN_ERR_NULL: i32 = -1;
pub const PDFGEN_ERR_UTF8: i32 = -2;
pub const PDFGEN_ERR_ARG: i32 = -3;
pub const PDFGEN_ERR_CORE: i32 = -4;
pub const PDFGEN_ERR_PANIC: i32 = -5;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(msg: impl Into<Vec<u8>>) {
    let c = CString::new(msg).unwrap_or_else(|e| {
        let v: Vec<u8> = e.into_vec().into_iter().filter(|&b| b != 0).collect();
        CString::new(v).unwrap()
    });
    // `try_with` fails instead of panicking once this thread's storage is destroyed (a pdfgen call from
    // another thread-local's destructor); a panic here would escape `guard` and cross the C boundary.
    let _ = LAST_ERROR.try_with(|l| *l.borrow_mut() = c);
}

fn fail(code: i32, msg: &str) -> i32 {
    set_error(msg);
    code
}

/// Runs `f`, converting panics to `PDFGEN_ERR_PANIC`.
fn guard(f: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(c) => c,
        Err(_) => fail(PDFGEN_ERR_PANIC, "internal panic"),
    }
}

unsafe fn cstr<'a>(p: *const c_char) -> Result<&'a str, i32> {
    if p.is_null() {
        return Err(fail(PDFGEN_ERR_NULL, "null string pointer"));
    }
    CStr::from_ptr(p)
        .to_str()
        .map_err(|_| fail(PDFGEN_ERR_UTF8, "string is not valid UTF-8"))
}

unsafe fn bytes<'a>(p: *const u8, len: usize) -> Result<&'a [u8], i32> {
    if p.is_null() && len != 0 {
        return Err(fail(PDFGEN_ERR_NULL, "null data pointer"));
    }
    Ok(if len == 0 { &[] } else { std::slice::from_raw_parts(p, len) })
}

fn font_from(code: i32) -> Result<Font, i32> {
    Font::ALL
        .get(usize::try_from(code).map_err(|_| fail(PDFGEN_ERR_ARG, "font code out of range"))?)
        .copied()
        .ok_or_else(|| fail(PDFGEN_ERR_ARG, "font code out of range"))
}

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(c) => return c,
        }
    };
}

/// Opaque document handle.
pub struct PdfgenDoc(Document);

/// Opaque table builder handle.
pub struct PdfgenTable(Table);

/// Last error message for this thread; never NULL. Do not free.
#[no_mangle]
pub extern "C" fn pdfgen_last_error() -> *const c_char {
    // After the thread's storage is destroyed there is no message; hand back an empty static string.
    LAST_ERROR.try_with(|l| l.borrow().as_ptr()).unwrap_or(b"\0".as_ptr().cast())
}

/// Library version, static NUL-terminated string. Do not free.
#[no_mangle]
pub extern "C" fn pdfgen_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

#[no_mangle]
pub extern "C" fn pdfgen_doc_new() -> *mut PdfgenDoc {
    Box::into_raw(Box::new(PdfgenDoc(Document::new())))
}

/// # Safety
/// `doc` must be NULL or a pointer from `pdfgen_doc_new` not yet freed.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_free(doc: *mut PdfgenDoc) {
    if !doc.is_null() {
        drop(Box::from_raw(doc));
    }
}

/// `font`: 0 Helvetica, 1 Helvetica-Bold, 2 Courier, 3 Courier-Bold.
///
/// # Safety
/// `doc` valid handle; `text` valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_add_text(doc: *mut PdfgenDoc, text: *const c_char, font: i32, size: f32) -> i32 {
    guard(|| {
        let doc = tri!(doc.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null doc")));
        let text = tri!(cstr(text));
        let font = tri!(font_from(font));
        if !(size.is_finite() && size > 0.0) {
            return fail(PDFGEN_ERR_ARG, "font size must be positive and finite");
        }
        doc.0.add_text(text, font, size);
        PDFGEN_OK
    })
}

/// # Safety
/// `doc` valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_add_page_break(doc: *mut PdfgenDoc) -> i32 {
    guard(|| {
        let doc = tri!(doc.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null doc")));
        doc.0.add_page_break();
        PDFGEN_OK
    })
}

fn check_width(width_pt: f32) -> Result<(), i32> {
    if width_pt.is_finite() && width_pt > 0.0 {
        Ok(())
    } else {
        Err(fail(PDFGEN_ERR_ARG, "image width must be positive and finite"))
    }
}

/// `channels`: 3 = 8-bit RGB, 1 = 8-bit gray; `len` must be `w * h * channels`.
///
/// # Safety
/// `doc` valid handle; `data` points to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_add_image_raw(
    doc: *mut PdfgenDoc,
    width: u32,
    height: u32,
    channels: u32,
    data: *const u8,
    len: usize,
    width_pt: f32,
) -> i32 {
    guard(|| {
        let doc = tri!(doc.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null doc")));
        tri!(check_width(width_pt));
        let data = tri!(bytes(data, len)).to_vec();
        let img = match channels {
            3 => Image::rgb(width, height, data),
            1 => Image::gray(width, height, data),
            _ => return fail(PDFGEN_ERR_ARG, "channels must be 1 or 3"),
        };
        match img {
            Ok(image) => {
                doc.0.add(Block::Image { image, width: width_pt });
                PDFGEN_OK
            }
            Err(e) => fail(PDFGEN_ERR_CORE, &e.to_string()),
        }
    })
}

/// # Safety
/// `doc` valid handle; `data` points to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_add_image_jpeg(doc: *mut PdfgenDoc, data: *const u8, len: usize, width_pt: f32) -> i32 {
    guard(|| {
        let doc = tri!(doc.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null doc")));
        tri!(check_width(width_pt));
        let data = tri!(bytes(data, len)).to_vec();
        match Image::jpeg(data) {
            Ok(image) => {
                doc.0.add(Block::Image { image, width: width_pt });
                PDFGEN_OK
            }
            Err(e) => fail(PDFGEN_ERR_CORE, &e.to_string()),
        }
    })
}

/// Creates a table with `ncols` columns of the given widths (points).
/// Returns NULL on error (see `pdfgen_last_error`).
///
/// # Safety
/// `widths` points to `ncols` readable floats.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_table_new(widths: *const f32, ncols: usize, font_size: f32) -> *mut PdfgenTable {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if widths.is_null() || ncols == 0 {
            set_error("table needs at least one column width");
            return ptr::null_mut();
        }
        let w = std::slice::from_raw_parts(widths, ncols);
        if !w.iter().all(|x| x.is_finite() && *x > 0.0) || !(font_size.is_finite() && font_size > 0.0) {
            set_error("column widths and font size must be positive and finite");
            return ptr::null_mut();
        }
        Box::into_raw(Box::new(PdfgenTable(Table {
            column_widths: w.to_vec(),
            header: None,
            rows: Vec::new(),
            font_size,
        })))
    }));
    r.unwrap_or_else(|_| {
        set_error("internal panic");
        ptr::null_mut()
    })
}

/// # Safety
/// `t` NULL or a live table handle.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_table_free(t: *mut PdfgenTable) {
    if !t.is_null() {
        drop(Box::from_raw(t));
    }
}

unsafe fn cells(cells: *const *const c_char, n: usize) -> Result<Vec<String>, i32> {
    if cells.is_null() && n != 0 {
        return Err(fail(PDFGEN_ERR_NULL, "null cells pointer"));
    }
    (0..n).map(|i| cstr(*cells.add(i)).map(str::to_owned)).collect()
}

/// # Safety
/// `t` live table; `cells` points to `n` valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_table_set_header(t: *mut PdfgenTable, cells_p: *const *const c_char, n: usize) -> i32 {
    guard(|| {
        let t = tri!(t.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null table")));
        t.0.header = Some(tri!(cells(cells_p, n)));
        PDFGEN_OK
    })
}

/// # Safety
/// `t` live table; `cells` points to `n` valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_table_add_row(t: *mut PdfgenTable, cells_p: *const *const c_char, n: usize) -> i32 {
    guard(|| {
        let t = tri!(t.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null table")));
        t.0.rows.push(tri!(cells(cells_p, n)));
        PDFGEN_OK
    })
}

/// Copies the table into the document; the caller still owns and must free `t`.
///
/// # Safety
/// `doc` and `t` live handles.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_add_table(doc: *mut PdfgenDoc, t: *const PdfgenTable) -> i32 {
    guard(|| {
        let doc = tri!(doc.as_mut().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null doc")));
        let t = tri!(t.as_ref().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null table")));
        doc.0.add_table(t.0.clone());
        PDFGEN_OK
    })
}

/// Renders the document. On success `*out_data` points to `*out_len` bytes that
/// must be released with `pdfgen_buffer_free(*out_data, *out_len)`.
/// On any failure (including NULL `doc`/out pointers and caught panics) every
/// non-NULL out pointer is set to NULL / 0 before returning the error code.
///
/// # Safety
/// `doc` live handle or NULL; `out_data` and `out_len` NULL or valid for writes.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_doc_render(doc: *const PdfgenDoc, out_data: *mut *mut u8, out_len: *mut usize) -> i32 {
    if !out_data.is_null() {
        *out_data = ptr::null_mut();
    }
    if !out_len.is_null() {
        *out_len = 0;
    }
    guard(|| {
        let doc = tri!(doc.as_ref().ok_or_else(|| fail(PDFGEN_ERR_NULL, "null doc")));
        if out_data.is_null() || out_len.is_null() {
            return fail(PDFGEN_ERR_NULL, "null output pointer");
        }
        match doc.0.render() {
            Ok(v) => {
                let b = v.into_boxed_slice();
                *out_len = b.len();
                *out_data = Box::into_raw(b).cast::<u8>();
                PDFGEN_OK
            }
            Err(e) => fail(PDFGEN_ERR_CORE, &e.to_string()),
        }
    })
}

/// # Safety
/// `data`/`len` exactly as returned by `pdfgen_doc_render`, freed once.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_buffer_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        drop(Box::from_raw(ptr::slice_from_raw_parts_mut(data, len)));
    }
}
