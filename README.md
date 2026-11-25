# pdfgen

Small PDF report generator in Rust with a C ABI and a .NET wrapper. It lays out
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
| `core/afm/` | Adobe AFM font metrics used for text widths, plus their license |
| `tools/gen_fontdata.py` | regenerates `core/src/fontdata.rs` from `core/afm/` |
| `deny.toml` | license allowlist for `cargo deny` |

## Prerequisites

| Tool | Needed for | Tested with |
|---|---|---|
| Rust (rustc + cargo) | everything | 1.100.0 |
| C linker (`cc`) | linking Rust binaries | gcc 12.2 |
| `qpdf` | the tests call `qpdf --check` on every PDF they produce | 11.3.0 |
| [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) | license check | 0.22.1 |
| .NET SDK 10 | the wrapper and its tests | 10.0.404 |
| Python 3 | only to regenerate font data | 3.11.2 |

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
# Rust: core + ffi tests (22 + 5). Needs qpdf on PATH.
cargo test --workspace

# License check against deny.toml.
cargo deny check licenses

# The native library the .NET tests load. It must exist before `dotnet test`.
cargo build --release -p ffi            # -> target/release/libpdfgen_ffi.so
```

The .NET build fails with a clear message if that library is missing. NuGet
restores 13 test-only packages (MIT / Apache-2.0). To keep the NuGet cache and
dotnet's per-user files inside this directory instead of `~/.nuget` and
`~/.dotnet`, set these first (`.dotnet-home/` is git-ignored):

```bash
export DOTNET_CLI_HOME="$PWD/.dotnet-home" NUGET_PACKAGES="$PWD/.dotnet-home/nuget"
cd dotnet && dotnet test
```

Do not override `HOME` for this: cargo and rustup look for the toolchain there and
fail with "no default toolchain" if it moves.

Test PDFs are left in `target/tmp/` (Rust) and next to the test assembly
(`dotnet/PdfGen.Tests/bin/`), so you can open them.

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
`Code`. `PdfDocument` is not thread-safe; use one per thread. The native library
must be next to your assembly or on the loader path; the test project copies it
from `target/release/`.

## C ABI

Library name `pdfgen_ffi`. There is no generated header; the functions are
declared in `ffi/src/lib.rs` and, as P/Invoke, in `dotnet/PdfGen/Native.cs`.

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
| `lopdf` 0.36 and its dependencies (tests only, dev-dependency) | MIT and/or Apache-2.0, checked by `cargo deny` |
| Arlington PDF Model TSV subset (`core/tests/arlington/`, tests only) | Apache-2.0, `LICENSE` and `NOTICE.txt` kept beside the files, source commit in `SOURCE.txt` |
| pdfcpu (optional external tool, not shipped) | Apache-2.0 |
| Adobe core-14 AFM files (`core/afm/`) | Adobe's own notice, in `core/afm/LICENSE`: use, copy and distribute for any purpose if all copyright notices are kept and the AFM files are never distributed without that `LICENSE` file |
| NuGet test packages (13) | MIT or Apache-2.0 |

`cargo deny check licenses` covers the Rust crates only. It cannot see the AFM
files or the NuGet packages. The built shared library links dynamically against
glibc and `libgcc_s` (`ldd target/release/libpdfgen_ffi.so`).
