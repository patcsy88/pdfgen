//! Runs the Arlington and content-stream checks over PDFs produced outside this
//! crate (the Node and .NET binding tests).
//!
//! `PDFGEN_CHECK_DIR=<dir>` names the directory; a relative path is resolved against
//! the workspace root. With the variable set, the test fails if the directory holds
//! no `.pdf` files or if any of them has a violation. Without it the test does
//! nothing, so a plain `cargo test --workspace` stays self-contained.

mod common;

use std::path::PathBuf;

#[test]
fn external_pdfs_are_conformant() {
    let Some(dir) = std::env::var_os("PDFGEN_CHECK_DIR") else {
        eprintln!("PDFGEN_CHECK_DIR not set; no external PDFs to check");
        return;
    };
    let mut dir = PathBuf::from(dir);
    if dir.is_relative() {
        dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(dir);
    }

    let mut pdfs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read PDFGEN_CHECK_DIR {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "pdf"))
        .collect();
    pdfs.sort();
    assert!(!pdfs.is_empty(), "no .pdf files in {}", dir.display());

    let mut failures = Vec::new();
    for p in &pdfs {
        let name = p.file_name().unwrap().to_string_lossy();
        let bytes = std::fs::read(p).unwrap();
        let mut errs = common::arlington_errors(&bytes);
        errs.extend(common::content::content_errors(&bytes));
        if errs.is_empty() {
            println!("ok {name}");
        } else {
            failures.push(format!("{name}: {} violation(s)\n    {}", errs.len(), errs.join("\n    ")));
        }
    }
    assert!(failures.is_empty(), "{} of {} PDFs failed:\n  {}", failures.len(), pdfs.len(), failures.join("\n  "));
    println!("{} PDF(s) checked", pdfs.len());
}
