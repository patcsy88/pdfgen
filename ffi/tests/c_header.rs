//! Compiles `ffi/include/pdfgen.h` through `tests/c/smoke.c` (as C99) and `tests/c/smoke.cpp` (as C++17)
//! with warnings as errors, links the result against the cdylib, runs it, and checks the PDF it wrote with
//! `qpdf --check`. Needs `cc`, `c++` and `qpdf` on PATH; a missing tool fails the test.
//!
//! The cdylib is not a dependency of this test binary, and where cargo puts it depends on the cargo
//! version, so the test asks cargo for the artifact path (`cargo build -p ffi --message-format=json`).
//! It builds the profile the test itself was built with, judged by `debug_assertions`; a custom profile
//! that changes that setting would pick the other one.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {:?}: {e}", cmd.get_program()));
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{:?} failed ({}):\n{text}", cmd.get_program(), out.status);
    text
}

/// Path of the shared library cargo built for the `ffi` package.
static CDYLIB: LazyLock<PathBuf> = LazyLock::new(|| {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "-p", "ffi", "--locked", "--message-format=json"]);
    if !cfg!(debug_assertions) {
        cmd.arg("--release");
    }
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    let out = cmd.output().expect("cannot run cargo");
    assert!(out.status.success(), "cargo build -p ffi failed:\n{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(r#""reason":"compiler-artifact""#) && l.contains(r#""name":"pdfgen_ffi""#))
        .flat_map(|l| l.split('"'))
        .filter(|s| s.contains("libpdfgen_ffi.") && (s.ends_with(".so") || s.ends_with(".dylib")))
        .map(PathBuf::from)
        .last()
        .expect("cargo reported no libpdfgen_ffi shared library")
});

fn build_and_run(compiler: &str, std_flag: &str, extra: &[&str], source: &str, name: &str) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let exe = tmp.join(format!("{name}-bin"));
    let pdf = tmp.join(format!("{name}.pdf"));
    let libs = CDYLIB.parent().unwrap();

    run(Command::new(compiler)
        .args([std_flag, "-Wall", "-Wextra", "-Wpedantic", "-Werror"])
        .args(extra)
        .arg("-I")
        .arg(manifest.join("include"))
        .arg(manifest.join("tests/c").join(source))
        .arg("-L")
        .arg(libs)
        .arg("-lpdfgen_ffi")
        .arg(format!("-Wl,-rpath,{}", libs.display()))
        .arg("-o")
        .arg(&exe));
    run(Command::new(&exe).arg(&pdf));

    let head = std::fs::read(&pdf).unwrap();
    assert!(head.starts_with(b"%PDF-"), "{name}: output is not a PDF");
    run(Command::new("qpdf").arg("--check").arg(&pdf));
    let pages = run(Command::new("qpdf").arg("--show-npages").arg(&pdf));
    assert_eq!(pages.trim(), "2", "{name}: one page break must give two pages");
}

#[test]
fn header_compiles_as_c99_and_runs() {
    build_and_run("cc", "-std=c99", &["-Wstrict-prototypes"], "smoke.c", "header-c99");
}

#[test]
fn header_compiles_as_cpp17_and_links_unmangled() {
    build_and_run("c++", "-std=c++17", &[], "smoke.cpp", "header-cpp17");
}
