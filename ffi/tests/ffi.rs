use pdfgen_ffi::*;
use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;
use std::process::Command;
use std::ptr;

fn last_error() -> String {
    unsafe { CStr::from_ptr(pdfgen_last_error()).to_string_lossy().into_owned() }
}

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

unsafe fn render(doc: *const PdfgenDoc) -> Vec<u8> {
    let mut data: *mut u8 = ptr::null_mut();
    let mut len = 0usize;
    assert_eq!(pdfgen_doc_render(doc, &mut data, &mut len), PDFGEN_OK, "{}", last_error());
    let v = std::slice::from_raw_parts(data, len).to_vec();
    pdfgen_buffer_free(data, len);
    v
}

fn qpdf_check(name: &str, bytes: &[u8]) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&p, bytes).unwrap();
    let r = Command::new("qpdf").args(["--check", p.to_str().unwrap()]).output().expect("qpdf installed");
    assert!(r.status.success(), "{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    p
}

fn npages(p: &PathBuf) -> usize {
    let r = Command::new("qpdf").args(["--show-npages", p.to_str().unwrap()]).output().unwrap();
    String::from_utf8_lossy(&r.stdout).trim().parse().unwrap()
}

#[test]
fn full_report_through_the_c_abi() {
    unsafe {
        let doc = pdfgen_doc_new();
        assert_eq!(pdfgen_doc_add_text(doc, cs("Title").as_ptr(), 1, 20.0), PDFGEN_OK);
        assert_eq!(pdfgen_doc_add_text(doc, cs("mono").as_ptr(), 2, 10.0), PDFGEN_OK);

        let widths = [60.0f32, 200.0];
        let t = pdfgen_table_new(widths.as_ptr(), 2, 10.0);
        assert!(!t.is_null());
        let h = [cs("Key"), cs("Value")];
        let hp: Vec<*const c_char> = h.iter().map(|c| c.as_ptr()).collect();
        assert_eq!(pdfgen_table_set_header(t, hp.as_ptr(), hp.len()), PDFGEN_OK);
        for i in 0..80 {
            let r = [cs(&format!("k{i}")), cs(&format!("value {i}"))];
            let rp: Vec<*const c_char> = r.iter().map(|c| c.as_ptr()).collect();
            assert_eq!(pdfgen_table_add_row(t, rp.as_ptr(), rp.len()), PDFGEN_OK);
        }
        assert_eq!(pdfgen_doc_add_table(doc, t), PDFGEN_OK);
        pdfgen_table_free(t);

        let px: Vec<u8> = (0..4 * 4 * 3).map(|i| i as u8 * 5).collect();
        assert_eq!(pdfgen_doc_add_image_raw(doc, 4, 4, 3, px.as_ptr(), px.len(), 80.0), PDFGEN_OK);
        assert_eq!(pdfgen_doc_add_page_break(doc), PDFGEN_OK);
        assert_eq!(pdfgen_doc_add_text(doc, cs("last page").as_ptr(), 0, 12.0), PDFGEN_OK);

        let bytes = render(doc);
        pdfgen_doc_free(doc);
        assert!(bytes.starts_with(b"%PDF-2.0"));
        let p = qpdf_check("ffi_report.pdf", &bytes);
        assert!(npages(&p) >= 3);
    }
}

#[test]
fn errors_are_codes_with_messages_not_panics() {
    unsafe {
        let doc = pdfgen_doc_new();
        assert_eq!(pdfgen_doc_add_text(ptr::null_mut(), cs("x").as_ptr(), 0, 12.0), PDFGEN_ERR_NULL);
        assert_eq!(pdfgen_doc_add_text(doc, ptr::null(), 0, 12.0), PDFGEN_ERR_NULL);
        let bad_utf8 = CString::new(vec![0xFFu8, 0xFE]).unwrap();
        assert_eq!(pdfgen_doc_add_text(doc, bad_utf8.as_ptr(), 0, 12.0), PDFGEN_ERR_UTF8);
        assert_eq!(pdfgen_doc_add_text(doc, cs("x").as_ptr(), 9, 12.0), PDFGEN_ERR_ARG);
        assert_eq!(pdfgen_doc_add_text(doc, cs("x").as_ptr(), -1, 12.0), PDFGEN_ERR_ARG);
        assert_eq!(pdfgen_doc_add_text(doc, cs("x").as_ptr(), 0, f32::NAN), PDFGEN_ERR_ARG);
        assert_eq!(pdfgen_doc_add_text(doc, cs("x").as_ptr(), 0, 0.0), PDFGEN_ERR_ARG);

        // Wrong buffer length is rejected by the core, surfaced as CORE with its message.
        let px = [0u8; 5];
        assert_eq!(pdfgen_doc_add_image_raw(doc, 2, 2, 3, px.as_ptr(), px.len(), 50.0), PDFGEN_ERR_CORE);
        assert!(last_error().contains("expected 12 bytes"), "{}", last_error());
        assert_eq!(pdfgen_doc_add_image_raw(doc, 2, 2, 4, px.as_ptr(), px.len(), 50.0), PDFGEN_ERR_ARG);
        assert_eq!(pdfgen_doc_add_image_jpeg(doc, b"nope".as_ptr(), 4, 50.0), PDFGEN_ERR_CORE);
        assert_eq!(pdfgen_doc_add_image_jpeg(doc, ptr::null(), 4, 50.0), PDFGEN_ERR_NULL);

        // Table row wider than the table: accepted when building, rejected at render.
        let w = [50.0f32];
        let t = pdfgen_table_new(w.as_ptr(), 1, 10.0);
        let r = [cs("a"), cs("b")];
        let rp: Vec<*const c_char> = r.iter().map(|c| c.as_ptr()).collect();
        assert_eq!(pdfgen_table_add_row(t, rp.as_ptr(), 2), PDFGEN_OK);
        assert_eq!(pdfgen_doc_add_table(doc, t), PDFGEN_OK);
        pdfgen_table_free(t);
        let sentinel = 0xdead as *mut u8;
        let (mut d, mut l) = (sentinel, 7usize);
        assert_eq!(pdfgen_doc_render(doc, &mut d, &mut l), PDFGEN_ERR_CORE);
        assert!(d.is_null() && l == 0);
        assert!(last_error().contains("more cells than columns"), "{}", last_error());

        // Sizes the core rejects at render time are accepted while building: the FFI only checks finite and > 0.
        let big = pdfgen_doc_new();
        assert_eq!(pdfgen_doc_add_text(big, cs("x").as_ptr(), 0, 5000.0), PDFGEN_OK);
        assert_eq!(pdfgen_doc_render(big, &mut d, &mut l), PDFGEN_ERR_CORE);
        assert!(d.is_null() && l == 0);
        assert!(last_error().contains("text size"), "{}", last_error());
        pdfgen_doc_free(big);

        assert!(pdfgen_table_new(ptr::null(), 0, 10.0).is_null());
        assert!(pdfgen_table_new(w.as_ptr(), 1, -1.0).is_null());
        // Failure clears whichever out-params are non-NULL, even on early NULL checks.
        let (mut d, mut l) = (sentinel, 7usize);
        assert_eq!(pdfgen_doc_render(ptr::null(), &mut d, &mut l), PDFGEN_ERR_NULL);
        assert!(d.is_null() && l == 0);
        d = sentinel;
        assert_eq!(pdfgen_doc_render(doc, &mut d, ptr::null_mut()), PDFGEN_ERR_NULL);
        assert!(d.is_null());
        l = 7;
        assert_eq!(pdfgen_doc_render(doc, ptr::null_mut(), &mut l), PDFGEN_ERR_NULL);
        assert_eq!(l, 0);

        pdfgen_doc_free(doc);
        pdfgen_doc_free(ptr::null_mut()); // no-op by contract
        pdfgen_table_free(ptr::null_mut());
        pdfgen_buffer_free(ptr::null_mut(), 0);
    }
}

#[test]
fn error_message_is_per_thread() {
    unsafe {
        assert_eq!(pdfgen_doc_add_page_break(ptr::null_mut()), PDFGEN_ERR_NULL);
        let main_msg = last_error();
        let other = std::thread::spawn(|| last_error()).join().unwrap();
        assert_eq!(main_msg, "null doc");
        assert_eq!(other, "", "a fresh thread has no error");
    }
}

// A pdfgen call from another thread-local's destructor can run after this library's own error slot is
// destroyed. Before `try_with`, that panicked twice (once in `set_error`, again in `guard`) and aborted.
#[test]
fn calls_from_a_thread_local_destructor_do_not_panic() {
    struct CallsOnDrop;
    impl Drop for CallsOnDrop {
        fn drop(&mut self) {
            unsafe {
                assert_eq!(pdfgen_doc_add_page_break(ptr::null_mut()), PDFGEN_ERR_NULL);
                // The slot is gone, so there is no message to return; the pointer must still be a valid empty C string.
                assert_eq!(CStr::from_ptr(pdfgen_last_error()).to_bytes(), b"");
            }
        }
    }
    thread_local! {
        static DROPPER: CallsOnDrop = const { CallsOnDrop };
    }
    std::thread::spawn(|| {
        // Registered first, so on Linux it is destroyed after the error slot created by the call below.
        DROPPER.with(|_| {});
        unsafe { pdfgen_doc_add_page_break(ptr::null_mut()) };
    })
    .join()
    .unwrap();
}

#[test]
fn jpeg_through_the_c_abi() {
    // Same minimal valid grayscale JPEG as the core test: 8x8, one flat block.
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00];
    v.extend_from_slice(&[1u8; 64]);
    v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00]);
    for class_id in [0x00u8, 0x10] {
        v.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, class_id, 0x01]);
        v.extend_from_slice(&[0u8; 15]);
        v.push(0x00);
    }
    v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x3F, 0xFF, 0xD9]);
    unsafe {
        let doc = pdfgen_doc_new();
        assert_eq!(pdfgen_doc_add_image_jpeg(doc, v.as_ptr(), v.len(), 64.0), PDFGEN_OK, "{}", last_error());
        let bytes = render(doc);
        pdfgen_doc_free(doc);
        qpdf_check("ffi_jpeg.pdf", &bytes);
    }
}

#[test]
fn version_string() {
    let v = unsafe { CStr::from_ptr(pdfgen_version()) }.to_str().unwrap();
    assert_eq!(v, env!("CARGO_PKG_VERSION"));
}
