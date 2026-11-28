# pdfgen

Small PDF report generator in Rust with a C ABI and wrappers for .NET, Node.js, Python, Java and Go. It lays out
wrapped text, tables (repeating header row across pages), RGB/gray/JPEG images
and explicit page breaks, and writes a PDF 2.0 (ISO 32000-2) file.

Licensed `MIT OR Apache-2.0` (see `LICENSE-MIT` and `LICENSE-APACHE`). The vendored
Adobe font metrics in `core/afm/` have their own terms; see [Licenses](#licenses).

| Path | What |
|---|---|
| `core/` | `pdfgen-core` crate: document model, layout, PDF writer |
| `ffi/` | `ffi` package, built as `libpdfgen_ffi.so` (`.dylib`, `.dll`): C ABI over the core |
| `dotnet/PdfGen/` | .NET 10 P/Invoke wrapper |
| `dotnet/PdfGen.Tests/` | xunit tests that call the native library through the wrapper |
| `node/` | Node.js wrapper (koffi) and its tests |
| `python/` | Python wrapper (`ctypes`) and its tests |
| `java/` | Java 25 wrapper (foreign function API) and its tests |
| `go/` | Go wrapper (cgo) and its tests |
| `core/afm/` | Adobe AFM font metrics used for text widths, plus their license |
| `tools/gen_fontdata.py` | regenerates `core/src/fontdata.rs` from `core/afm/` |
| `deny.toml` | license allowlist, advisory, ban and source policy for `cargo deny` |

## Key features

- **Layout:** wrapped text, tables whose header row repeats across pages, RGB, gray and JPEG images, and
  explicit page breaks, on A4 pages with 50 pt margins. The output is PDF 2.0 and byte-identical for
  identical input.
- **Fonts:** Helvetica, Helvetica-Bold, Courier and Courier-Bold in WinAnsi (ASCII and Latin-1; any other
  character is written as `?`). Nothing is embedded; widths come from Adobe's AFM files.
- **One C ABI, five wrappers:** .NET, Node.js, Python, Java and Go call the same `libpdfgen_ffi`. Python and
  Go use only their standard library; the Node wrapper's one dependency is koffi.
- **Errors instead of broken PDFs:** out-of-range sizes, widths, tables and images are rejected with an error
  and never written (limits in the table below).
- **Validated output:** every test PDF goes through `qpdf --check`, the Arlington model and content-stream
  checks, and seeded random-input tests. CI runs Rust, .NET, Node.js (24 and 26), Python (3.10 and 3.13), Java and Go
  (1.24 and stable) on pushes to `main` and on pull requests.
- **Source release only:** nothing is published to crates.io, NuGet, npm, PyPI or Maven. Go clients use
  `go get github.com/patcsy88/pdfgen/go@v0.2.1`.

### Release history

The latest release is [v0.2.1](https://github.com/patcsy88/pdfgen/releases/tag/v0.2.1) (2026-11-28). Earlier ones are
[v0.2.0](https://github.com/patcsy88/pdfgen/releases/tag/v0.2.0) (2026-11-28) and
[v0.1.1](https://github.com/patcsy88/pdfgen/releases/tag/v0.1.1) (2026-11-27). v0.2.0 was a minor release
because some input that v0.1.1 rendered is now rejected: see the limits in the first row. v0.2.1 adds no new
limits. `git log v0.2.1..HEAD` lists anything pushed to `main` since.

| Area | In v0.1.1 | Added in v0.2.0 | Added in v0.2.1 |
|---|---|---|---|
| Input limits | Text size at most 618.33 pt; column width above 8 pt; NaN, infinite and non-positive sizes and widths rejected; a table row taller than a page together with its header (`RowTooTall`); non-8-bit JPEG | Text and table font size at least 1; at most 256 columns; at most 10000 pages; at most 256 MiB of page content plus image data; JPEG at most 100 Mpx | |
| JPEG | 8-bit baseline or progressive gray/RGB, embedded unchanged | Frame header length checked, an SOS marker required; second or unsupported frame headers and short segments rejected | |
| Layout | | | A table header moves to the next page with its first row, so it is no longer drawn alone at the bottom of a page and repeated above that row |
| Memory | | Wrapping stops at what a page can hold, so a huge cell no longer allocates every line first; image bytes count toward the cap; a tiny image keeps a valid matrix | The content cap is checked after every line of a table cell, so one huge cell can pass it by about a line instead of a whole cell |
| Native library lookup | | Python and Node use the `target/release` default only in a source checkout; `PDFGEN_LIB` is made absolute in Python, Node and Java; .NET loads from the assembly directory only | Python and Node use the default only for the real `<repo>/python/pdfgen` and `<repo>/node/src` layouts; a vendored copy anywhere else needs `PDFGEN_LIB` |
| Handles and I/O | Go and Java no longer free a handle during a native call | Native handles guarded against copy and close races; Python `save()` keeps an existing file when the render fails; hostile `bytes`, `Uint8Array` or `Array` subclasses can no longer desync a length; .NET throws `ArgumentNullException` for a null row | |
| Dependency policy | | `cargo deny check` (licenses, advisories, bans, sources) in CI | |
| CI | | | Node tests on 24 and 26; checkouts do not persist the token; `tools/gen_fontdata.py --check` runs |

## Prerequisites

| Tool | Needed for | Tested with |
|---|---|---|
| Rust (rustc + cargo) | everything | 1.100.0 |
| C and C++ compiler (`cc`, `c++`) | linking Rust binaries; the header test in `ffi/tests/c_header.rs` compiles C99 and C++17 | gcc and g++ 12.2 |
| `qpdf` | the tests call `qpdf --check` on every PDF they produce | 11.3.0 |
| [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) | license, advisory, ban and source checks | 0.22.1 |
| .NET SDK 10 | the wrapper and its tests | 10.0.404 |
| Node.js | the Node wrapper and its tests (`engines` is `>=24`) | 26.13.1 locally; CI runs 24 and 26: see [Use from Node.js](#use-from-nodejs) |
| Python 3 | the Python wrapper and its tests (`requires-python` is `>=3.10`); also regenerates font data | 3.11.2 |
| JDK 25 | the Java wrapper and its tests (`maven.compiler.release` is 25) | 25.0.5.1 (Microsoft build) |
| Maven | building and testing `java/` | 3.8.7 |
| Go | the Go wrapper and its tests (`go.mod` says `go 1.24`; needs `cc` for cgo) | 1.27.4 |

Debian/Ubuntu:

```bash
sudo apt install build-essential qpdf
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # then restart the shell
cargo install cargo-deny --locked
# .NET 10: add Microsoft's package feed, then
sudo apt install dotnet-sdk-10.0
```

.NET 8 went out of support on 2026-11-10; the projects target `net10.0`.

`Cargo.toml` declares `rust-version = "1.75"`. That number was chosen, not
measured: nothing here has been built with a compiler older than 1.100.

## Build and test

All commands run from the repository root.

```bash
# Rust: core + ffi tests (43 + 8). Needs qpdf on PATH.
cargo test --workspace

# License, advisory, ban and source checks against deny.toml.
cargo deny check

# The native library the .NET tests load. It must exist before `dotnet test`.
cargo build --release -p ffi            # -> target/release/libpdfgen_ffi.so
```

```bash
# Node.js: needs the release library above and qpdf on PATH.
cd node && npm ci && npm test           # writes node/test-output/*.pdf

# Arlington + content-stream checks over those PDFs (fails if the directory is empty).
cd .. && PDFGEN_CHECK_DIR=node/test-output cargo test -p pdfgen-core --test external_pdfs

# npm licenses: every package in the lockfile against the seven-license allowlist.
cd node && npm run license-check

# Python: needs the release library above and qpdf on PATH. Standard library only.
cd python && python3 -m unittest discover -s tests   # writes python/test-output/*.pdf
cd .. && PDFGEN_CHECK_DIR=python/test-output cargo test -p pdfgen-core --test external_pdfs

# Java: needs the release library above, JDK 25, Maven and qpdf on PATH.
cd java && mvn test                                  # writes java/test-output/*.pdf
cd .. && PDFGEN_CHECK_DIR=java/test-output cargo test -p pdfgen-core --test external_pdfs
cd java && python3 license-check.py                  # every Maven package, test scope included

# Go: needs the release library above, Go, a C compiler (cgo) and qpdf on PATH. Standard library only.
cd go && LD_LIBRARY_PATH=../target/release go test ./...   # writes go/test-output/*.pdf
cd .. && PDFGEN_CHECK_DIR=go/test-output cargo test -p pdfgen-core --test external_pdfs
cd go && ./license-check.sh                          # fails if any third-party module appears
python3 tools/abi-check.py                           # every wrapper declares the same C ABI as ffi/src/lib.rs
python3 tools/version-check.py                       # every manifest carries the workspace version in Cargo.toml
```

The .NET build fails with a clear message if that library is missing. NuGet
restores 13 test-only packages (MIT / Apache-2.0; listed in `dotnet/NOTICE.txt`). To keep the NuGet cache and
dotnet's per-user files inside this directory instead of `~/.nuget` and
`~/.dotnet`, set these first (`.dotnet-home/` is git-ignored):

```bash
export DOTNET_CLI_HOME="$PWD/.dotnet-home" NUGET_PACKAGES="$PWD/.dotnet-home/nuget"
cd dotnet && dotnet test
```

Do not override `HOME` for this: cargo and rustup look for the toolchain there and
fail with "no default toolchain" if it moves.

Test PDFs are left in `target/test-pdfs/` (Rust) and next to the test assembly
(`dotnet/PdfGen.Tests/bin/`), so you can open them.

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `main`, on pushes of tags `v*` and `go/v*`,
and on pull requests: one job each for Rust (tests, `tools/abi-check.py`,
`tools/version-check.py`, `tools/gen_fontdata.py --check`, `cargo deny check`), .NET, Node.js (24 and 26), Python (3.10 and
3.13), Java and Go (1.24 and stable). Each wrapper job builds the release library, runs
the wrapper's tests and, except .NET, the `external_pdfs` checks.
Every action is pinned to a commit SHA. The workflow passes `actionlint` and runs green on GitHub,
including the Temurin JDK 25 build, Python 3.10, and Node 24 and 26.

## Releasing

The version is `version` under `[workspace.package]` in `Cargo.toml`. To release, change
it there and in `Cargo.lock`, `node/package.json`, both version fields of
`node/package-lock.json`, `python/pyproject.toml`, the project's own `<version>` in
`java/pom.xml` and `<Version>` in `dotnet/PdfGen/PdfGen.csproj`. `core` and `ffi` inherit
it. Then run `python3 tools/version-check.py`; it exits 1 and lists every site that
differs. Nothing bumps the versions for you.

The Go module has no version in its source. Go clients take it from a git tag, and since
the module lives in `go/`, that tag must be `go/vX.Y.Z`. Push `vX.Y.Z` and `go/vX.Y.Z`
on the same commit; CI runs `tools/version-check.py --tag` on each and fails if the
tag differs from `Cargo.toml`. The `pdfgen.h` comment shows `0.1.0` only as an example
and is not checked. No package is published to crates.io, NuGet, npm, PyPI or Maven by
anything in this repository.

## Use from Rust

```rust
use pdfgen_core::{Document, Font, Image, Table};

fn main() -> Result<(), pdfgen_core::Error> {
    let mut doc = Document::new();
    doc.add_text("Quarterly report", Font::HelveticaBold, 20.0)
        .add_text("Totals by region.", Font::Helvetica, 11.0)
        .add_table(Table {
            column_widths: vec![80.0, 120.0],
            header: Some(vec!["Region".into(), "Total".into()]),
            rows: vec![vec!["North".into(), "1200".into()], vec!["South".into(), "900".into()]],
            font_size: 10.0,
        })
        .add_image(Image::gray(2, 2, vec![0, 85, 170, 255])?, 60.0)
        .add_page_break()
        .add_text("Appendix", Font::Courier, 10.0);
    std::fs::write("report.pdf", doc.render()?).unwrap();
    Ok(())
}
```

Page size is A4 (595 x 842 pt) with 50 pt margins. `Image::rgb` and `Image::gray`
take 8-bit pixels; `Image::jpeg` embeds a baseline or progressive gray/RGB JPEG
unchanged (CMYK is rejected).

### Text and fonts

Fonts are the four standard faces `Helvetica`, `Helvetica-Bold`, `Courier` and
`Courier-Bold`. None is embedded; the viewer supplies them. Text is encoded as
WinAnsi, so ASCII and Latin-1 work and every other character is written as `?`.
Widths come from Adobe's AFM files and are also written into each font's
`/Widths` array, so layout and the PDF agree.

## Use from .NET

```csharp
using PdfGen;

using var doc = new PdfDocument();
doc.AddText("Quarterly report", PdfFont.HelveticaBold, 20f);

using var table = new PdfTable(new[] { 80f, 120f }, 10f);
table.SetHeader("Region", "Total").AddRow("North", "1200");
doc.AddTable(table);          // copies the table; you still dispose it

doc.Save("report.pdf");       // or: byte[] pdf = doc.Render();
```

Failures throw `PdfException` with the native message and the C error code in
`Code`. `PdfDocument` is not thread-safe for concurrent builds; use one per thread. `Dispose` and the
finalizer wait for a native call in flight on another thread. The native library
must be next to your assembly: it is loaded from the assembly directory only, not from the loader path
or `PATH` (on Linux a copy on `LD_LIBRARY_PATH` is not found). The test project copies it
from `target/release/`.

## Use from Python

The wrapper in `python/pdfgen/` loads `libpdfgen_ffi.so` with `ctypes` (standard
library, no third-party dependencies), the same C ABI the .NET and Node wrappers use.

```python
from pdfgen import PdfDocument, PdfTable, Font

with PdfDocument() as doc:
    doc.add_text("Quarterly report", Font.HELVETICA_BOLD, 20)

    with PdfTable([80, 120], 10) as table:
        table.set_header("Region", "Total").add_row("North", "1200")
        doc.add_table(table)          # copies the table; you still close it

    doc.save("report.pdf")            # or: pdf_bytes = doc.render()
```

Failures raise `PdfError` with the native message and the C error code in `code`.
Arguments ctypes would silently mangle (NUL in a string, an out-of-range unsigned
integer) raise `ValueError` before reaching the library. Use `with` or `close()` to
free native memory; a finalizer frees anything left over. A closed object raises
`ValueError`. The library path defaults to `target/release/libpdfgen_ffi.so` in this
repository, found relative to the package source, and only when the package sits in a source
checkout; any other copy raises `OSError` on import. Run from a checkout (for example
`PYTHONPATH=python`), or set `PDFGEN_LIB` to load the library from elsewhere (a relative path is
resolved against the current directory); a `pip install ./python` copy has no repository next to it and
needs `PDFGEN_LIB`. CI runs the tests on Python 3.10 and 3.13; locally they ran on Python 3.11.2.

The Python tests write every PDF to `python/test-output/` (git-ignored) and require
`qpdf --check` to pass on each. The Arlington and content-stream checks run
separately over that directory with the `external_pdfs` test shown under Build and
test.

## Use from Java

The wrapper in `java/` calls `libpdfgen_ffi.so` through the Panama foreign function
API (`java.lang.foreign`, final since Java 22), the same C ABI the other wrappers
use. It has no runtime dependencies; TestNG is test scope only.

```java
import io.github.patcsy88.pdfgen.*;

try (PdfDocument doc = new PdfDocument()) {
    doc.addText("Quarterly report", Font.HELVETICA_BOLD, 20);

    try (PdfTable table = new PdfTable(new float[] {80, 120}, 10)) {
        table.setHeader("Region", "Total").addRow("North", "1200");
        doc.addTable(table);          // copies the table; you still close it
    }

    doc.save(Path.of("report.pdf"));  // or: byte[] pdf = doc.render();
}
```

Failures throw `PdfException` with the native message and the C error code in
`code()`. Arguments the C layer would mangle (a NUL in a string, a negative value for
an unsigned parameter) throw `IllegalArgumentException` before the call, and null
arguments throw `NullPointerException`. Use try-with-resources or `close()` to free
native memory; a `Cleaner` frees anything left over. A closed object throws
`IllegalStateException`. A document or table is not thread-safe for concurrent builds; use one per thread.
`close()` waits for a native call in flight on another thread.

The library is not searched for: set `PDFGEN_LIB` to its path (a relative path is resolved against the
current directory), or the first call throws
`UnsatisfiedLinkError`. Surefire sets it to `target/release/libpdfgen_ffi.so`; override with
`-Dpdfgen.lib=...`. The Surefire
configuration passes `--enable-native-access=ALL-UNNAMED` to the JVM; running without
that flag was not tested. The build sets `maven.compiler.release` to 25, and the tests
ran on JDK 25 only (Temurin in CI, Microsoft build 25.0.5.1 locally); the API is final from Java 22, but 22 to 24 are untested. On
Debian with Microsoft's package feed: `sudo apt install msopenjdk-25 maven`.

The Java tests (27) write every PDF to `java/test-output/` (git-ignored) and require
`qpdf --check` to pass on each. The Arlington and content-stream checks run
separately over that directory with the `external_pdfs` test shown under Build and
test. `license-check.py` reads each resolved package's license from its POM and
exits 1 unless it is one of the seven allowed licenses.

## Use from Go

The module in `go/` links `libpdfgen_ffi.so` through cgo, the same C ABI the other
wrappers use. It has no third-party modules: `go.mod` has no `require` lines and there
is no `go.sum`.

```go
import pdfgen "github.com/patcsy88/pdfgen/go"

doc, err := pdfgen.NewDocument()
if err != nil {
	log.Fatal(err)
}
defer doc.Close()

doc.AddText("Quarterly report", pdfgen.HelveticaBold, 20)

table, _ := pdfgen.NewTable([]float32{80, 120}, 10)
defer table.Close()
table.SetHeader("Region", "Total")
table.AddRow("North", "1200")
doc.AddTable(table) // copies the table; you still close it

if err := doc.Save("report.pdf"); err != nil { // or: pdf, err := doc.Render()
	log.Fatal(err)
}
```

Every method that can fail returns an `error`; check it (the example drops some for
brevity). Native failures are `*pdfgen.Error` with the native message and the C error
code in `Code`. A NUL in a string is rejected before the call, and a closed object
returns `pdfgen.ErrClosed`. `Close` is safe to call twice, and a cleanup frees anything
left unclosed. A document or table must not be built from several goroutines at once; use one per
goroutine. `Close` is safe from any goroutine, waits for a call in flight, and copying a `Document` or
`Table` value shares one handle instead of double-freeing.

The library keeps its last error message per OS thread, so each failing call and the
read of its message run on one locked OS thread. The test
`TestErrorMessagesDoNotLeakBetweenGoroutines` exercises this under 16 goroutines.

The cgo link flags add `target/release/` of this repository to the linker search path
only. They set no rpath, which would bake this build directory into every binary and let
the loader pick it over system directories. `go test` and any binary built here need the
library on the loader path at run time, e.g. `LD_LIBRARY_PATH=target/release`.
The tests ran on Go 1.27.4 locally, also under `-race`; CI runs Go 1.24, the `go 1.24` directive's floor, and the
current stable release. Only Linux is declared in the cgo flags.

The Go tests (28) write every PDF to `go/test-output/` (git-ignored) and require
`qpdf --check` to pass on each. The Arlington and content-stream checks run
separately over that directory with the `external_pdfs` test shown under Build and
test.

## Use from Node.js

The wrapper in `node/` loads `libpdfgen_ffi.so` through [koffi](https://koffi.dev)
(MIT), the same C ABI the .NET wrapper uses. It is an ES module with no build step.

```js
import { writeFileSync } from 'node:fs';
import { PdfDocument, PdfTable, Font } from './node/src/index.js';

using doc = new PdfDocument();
doc.addText('Quarterly report', Font.HelveticaBold, 20);

using table = new PdfTable([80, 120], 10);
table.setHeader('Region', 'Total').addRow('North', '1200');
doc.addTable(table);              // copies the table; you still dispose it

writeFileSync('report.pdf', doc.render());   // render() returns a Buffer
```

Failures throw `PdfError` with the native message and the C error code in `code`.
A non-string argument, or a string containing a NUL (which the C layer would
silently truncate), throws `TypeError` before the call.
Call `dispose()` (or use `using`) to free native memory; a finalizer frees anything
left over. The library path defaults to `target/release/libpdfgen_ffi.so` in this
repository, and only when the package sits in a source checkout (importing it from any other copy
throws); set `PDFGEN_LIB` to load it from elsewhere (a relative path is resolved against the current
directory).

CI runs the Node tests on Node 24 and 26; locally they ran on Node 26.13.1. `package.json` declares
`"node": ">=24"`.

The Node tests write every PDF to `node/test-output/` (git-ignored) and require
`qpdf --check` to pass on each. The Arlington and content-stream checks are Rust
code, so they run separately over that directory with the `external_pdfs` test shown
under Build and test.

## C ABI

Library name `pdfgen_ffi`. C and C++ programs include `ffi/include/pdfgen.h`, which is
written by hand: it declares every exported function, the `PDFGEN_ERR_*` codes and the
`PDFGEN_FONT_*` codes, and documents ownership and error behavior. Link with
`-L target/release -lpdfgen_ffi`. The header has been compiled as C99 and C++17 with
`-Wall -Wextra -Wpedantic -Werror` by gcc 12.2 only; it has not been built on Windows
or macOS, or with clang or MSVC.

`ffi/tests/c_header.rs` compiles `ffi/tests/c/smoke.c` against the header as C99 and as
C++17, links it to the library cargo just built, runs it, and checks the PDF it writes
with `qpdf --check`. It runs under `cargo test --workspace`.

The functions are also declared in `ffi/src/lib.rs` and, as P/Invoke, in
`dotnet/PdfGen/Native.cs`, for koffi in `node/src/native.js`, for ctypes in
`python/pdfgen/_native.py`, for the Java foreign function API in
`java/src/main/java/io/github/patcsy88/pdfgen/Native.java`, and for cgo in the comment
above `import "C"` in `go/pdfgen.go`.

`tools/abi-check.py` compares the name, argument count and argument widths (pointer,
32-bit, 64-bit, float) of every function across the Rust source, the header and the
five wrapper declarations, and exits 1 on any difference. For the header only, it also
compares the `PDFGEN_*` values with the constants in `ffi/src/lib.rs` and the order of
`Font::ALL`. It does not compare signedness or pointee types, and it does not check the
constants that each wrapper declares for itself.

- Handles are opaque pointers. Free each with its matching `*_free`; passing NULL
  to a `*_free` function is a no-op.
- Functions returning `int` return `0` on success and a negative code on failure:
  `-1` null pointer, `-2` invalid UTF-8, `-3` bad argument, `-4` error from the
  core (for example a row with more cells than columns, reported at render),
  `-5` internal panic (panics never cross the boundary).
- `pdfgen_last_error()` returns the message for the calling thread, valid until the
  next call on that thread. Do not free it.
- Strings are NUL-terminated UTF-8. Font codes: `0` Helvetica, `1` Helvetica-Bold,
  `2` Courier, `3` Courier-Bold.
- `pdfgen_doc_render` hands back a buffer; release it with
  `pdfgen_buffer_free(data, len)` using the same length.

| Function | Purpose |
|---|---|
| `pdfgen_version()` | static version string |
| `pdfgen_doc_new()` / `pdfgen_doc_free(doc)` | create / destroy a document |
| `pdfgen_doc_add_text(doc, text, font, size)` | add wrapped text |
| `pdfgen_doc_add_page_break(doc)` | force a new page |
| `pdfgen_doc_add_image_raw(doc, w, h, channels, data, len, width_pt)` | 8-bit RGB (`3`) or gray (`1`) |
| `pdfgen_doc_add_image_jpeg(doc, data, len, width_pt)` | embed a JPEG |
| `pdfgen_table_new(widths, ncols, font_size)` / `pdfgen_table_free(t)` | table builder; NULL on error |
| `pdfgen_table_set_header(t, cells, n)` / `pdfgen_table_add_row(t, cells, n)` | fill the table |
| `pdfgen_doc_add_table(doc, t)` | copy the table into the document |
| `pdfgen_doc_render(doc, &data, &len)` / `pdfgen_buffer_free(data, len)` | produce the PDF bytes |

## Regenerating font data

`core/src/fontdata.rs` is generated. After changing `core/afm/` or the font list in
`tools/gen_fontdata.py`:

```bash
python3 tools/gen_fontdata.py           # rewrite the file
python3 tools/gen_fontdata.py --check   # exit 1 with a diff if it is stale
```

The script refuses to run if `core/afm/LICENSE` is missing. Adding a font also
means adding it to `Font` and `Font::metrics` in `core/src/lib.rs`.

## What is and is not validated

Every PDF the tests produce (written to `target/test-pdfs/`) goes through three
layers, all inside `cargo test`, plus an optional fourth:

1. **`qpdf --check`**: syntax, xref, stream lengths, stream decoding.
2. **Arlington PDF Model** (`core/tests/common/mod.rs`): the PDF is parsed with
   `lopdf` and walked from the trailer. Each object is checked against the PDF
   Association's machine-readable model of ISO 32000-2, vendored in
   `core/tests/arlington/`. The checks cover key types, required keys (with
   inheritance), direct versus indirect references, allowed values,
   `SinceVersion` and `DeprecatedIn` against PDF 2.0, keys the model doesn't
   define, and special-case predicates. A predicate the checker can't evaluate, a
   link to a model that isn't vendored, or an object not reachable from the
   trailer is reported as an error, never skipped.
3. **Content streams** (`core/tests/common/content.rs`): each operator is checked
   against the PDF operator table, with operand count and types, the
   graphics-object state machine (ISO 32000-2 Figure 9), and balanced `q`/`Q`,
   `BT`/`ET` and marked content. Resource names for `Tf`, `Do`, `gs`, `cs`, `sh`
   and `BDC` must resolve, and shown bytes must fall within the font's
   `FirstChar`..`LastChar`.
4. **pdfcpu strict mode** (optional, needs pdfcpu installed): run
   `tools/validate-pdfcpu.sh` after `cargo test`.

Both checkers have tests of their own: valid files that must pass and seeded
violations that must be reported.

### Randomised input tests

`core/tests/fuzz.rs` is part of `cargo test`. It is a seeded generator, not a
coverage-guided fuzzer: it builds random documents (text with parentheses,
backslashes, control characters and non-Latin-1 characters; tables with zero
columns, too many cells and very narrow columns; raw and JPEG images; page breaks;
and NaN, infinite, zero, negative and huge sizes and widths) and 10 times as many
mutated JPEG headers. For every input `render` must not panic, must return `Ok` or
one of the documented errors, and every file it returns must pass layers 2 and 3
above. One document in 20 also goes through `qpdf --check`, except documents that
hold a header-only JPEG, which qpdf cannot decode.

The defaults (seed 1, 300 documents, 3000 JPEG mutations) take about 12 s in a debug
build. A failure prints the seed and iteration. For a longer run:

```bash
PDFGEN_FUZZ_SEED=7 PDFGEN_FUZZ_ITERS=3000 cargo test -p pdfgen-core --release --test fuzz
```

Its first run found that `render` wrote `NaN`, `inf`, 1e30 coordinates and negative
scales into content streams when a text size, column width or image width had such a
value; the content-stream check reported them. `render` now returns
`Error::BadDimension` instead; the limits are in the `Document::render` doc comment
and `ffi/include/pdfgen.h`. These checks happen at render time, so
`pdfgen_doc_add_text` and the wrappers still accept a size such as 5000 and fail
later, in `render`, with the code for a core error.

What it does not do: it does not explore code paths by coverage, it does not fuzz the
C ABI (null and dangling pointers, bad lengths) or any wrapper, and no input comes
from a real-world corpus. It has run only on Linux x86-64 with Rust 1.100.0.

Limits, so "passes" isn't read as "certified":

- **Model subset:** only the 16 Arlington models reachable from objects this
  writer emits are vendored. A new object kind fails the check until its model is
  added.
- **Arlington's own scope:** Arlington doesn't cover every prose rule in ISO
  32000-2.
- **Operator table:** written from the standard's operator summary, not checked
  line by line against the normative text.
- **pdfcpu:** its documentation calls its PDF 2.0 validation "rudimentary".
- **Rendering:** not tested.
- **JPEG decode:** only a grayscale JPEG goes through the full `qpdf` decode;
  RGB JPEG is tested for header parsing only.

### Installing pdfcpu (optional)

pdfcpu 0.16.4 needs Go 1.26. Debian 12 ships Go 1.19 (1.23 in backports), so
use the prebuilt release instead of apt:

```bash
curl -LO https://github.com/pdfcpu/pdfcpu/releases/download/v0.16.4/pdfcpu_0.16.4_Linux_x86_64.tar.xz
curl -LO https://github.com/pdfcpu/pdfcpu/releases/download/v0.16.4/checksums.txt
sha256sum --ignore-missing -c checksums.txt
tar -xf pdfcpu_0.16.4_Linux_x86_64.tar.xz
install -D "$(find . -name pdfcpu -type f -perm -u+x | head -1)" ~/.local/bin/pdfcpu
pdfcpu version
```

## Licenses

| Component | License |
|---|---|
| pdfgen (this repository) | MIT OR Apache-2.0 |
| `miniz_oxide` 0.8.9 (Flate compression) | MIT OR Zlib OR Apache-2.0 |
| `adler2` 2.0.1 (via `miniz_oxide`) | 0BSD OR MIT OR Apache-2.0 |
| `lopdf` 0.42.0 (tests only, dev-dependency) | MIT |
| `lopdf`'s dependencies (tests only), including `miniz_oxide` 0.9.1 | mostly MIT and/or Apache-2.0; `encoding_rs` also BSD-3-Clause and the `unicode-*` crates also Unicode-3.0; checked by `cargo deny` |
| Arlington PDF Model TSV subset (`core/tests/arlington/`, tests only) | Apache-2.0, `LICENSE` and `NOTICE.txt` kept beside the files, source commit in `SOURCE.txt` |
| pdfcpu (optional external tool, not shipped) | Apache-2.0 |
| Adobe core-14 AFM files (`core/afm/`) | Adobe's own notice, in `core/afm/LICENSE`: use, copy and distribute for any purpose if all copyright notices are kept and the AFM files are never distributed without that `LICENSE` file |
| NuGet test packages (13) | MIT or Apache-2.0; see `dotnet/NOTICE.txt` (`xunit.abstractions` 2.0.3 is Apache-2.0 by reference, see there) |
| Python wrapper (`python/`) | no third-party packages; standard-library `ctypes` only |
| Java wrapper (`java/`) | no runtime dependencies; `java.lang.foreign` only |
| Maven test packages (4: `testng` 7.11.0, `jcommander` 1.83, `slf4j-api` 2.0.16, `jquery` 3.7.1) | Apache-2.0, Apache-2.0, MIT, MIT; test scope only, none shipped |
| Go wrapper (`go/`) | no third-party modules; standard library and cgo only |
| `koffi` 3.3.8 (Node wrapper, shipped dependency) | MIT |
| `@koromix/koffi-linux-x64` 3.3.8 (koffi's prebuilt binary, shipped dependency) | MIT |

`cargo deny check licenses` covers the Rust crates only. It cannot see the AFM
files, the NuGet packages, the npm packages or the Maven packages. The Maven build
plugins are not covered by `java/license-check.py`. `go/license-check.sh` only
confirms that the Go module has no third-party modules. The built shared library
links dynamically against glibc and `libgcc_s` (`ldd target/release/libpdfgen_ffi.so`).

`node/license-check.mjs` reads the `license` field of every package in
`package-lock.json` (`koffi` and its 20 optional platform binaries, 21 in all) and
exits 0 only if each is on the seven-license allowlist. It trusts the registry's
field; it does not read license files. An `AND`, `WITH` or missing license fails. The
node project has no dev dependencies.
