# Vendored Adobe core-14 AFM files

`Helvetica.afm`, `Helvetica-Bold.afm`, `Courier.afm`, `Courier-Bold.afm` and
`LICENSE` are unmodified copies from
<https://github.com/tecnickcom/tc-font-core14-afms> (a mirror of Adobe's
`Core14_AFMs.zip`).

The `LICENSE` file permits use, copying and distribution for any purpose and
without charge, with or without modification, provided that all copyright
notices are retained, that the AFM files are not distributed without that
`LICENSE`, that modifications are prominently noted, and that its paragraph is
not modified. Keep `LICENSE` next to the `.afm` files.

This is Adobe's own notice, not an SPDX-listed licence, so `cargo deny` does not
see it. The AFM files are not a Cargo dependency.

## Regenerating `../src/fontdata.rs`

`fontdata.rs` holds, for each font, the width of every WinAnsi code 32..=126 and
160..=255 (looked up by Adobe glyph name) plus FontBBox, Ascender, Descender,
CapHeight, ItalicAngle and StdVW from the AFM header. Codes outside those ranges
are 0. Regenerate it with `python3 tools/gen_fontdata.py` (add `--check` to only
compare); never edit it by hand.
