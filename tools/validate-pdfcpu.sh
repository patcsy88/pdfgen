#!/usr/bin/env bash
# Second-opinion validation: runs `pdfcpu validate --mode strict` over every PDF the
# core tests wrote to target/test-pdfs/. Run `cargo test -p pdfgen-core` first.
# pdfcpu (Apache-2.0) is not a Cargo dependency; install it as described in README.md.
# Exits non-zero if pdfcpu is missing, no PDFs exist, or any file fails validation.
set -euo pipefail
cd "$(dirname "$0")/.."

PDFCPU="${PDFCPU:-pdfcpu}"
if ! command -v "$PDFCPU" >/dev/null; then
    echo "error: pdfcpu not found (set PDFCPU=/path/to/pdfcpu or put it on PATH)" >&2
    exit 2
fi

shopt -s nullglob
files=()
for f in target/test-pdfs/*.pdf; do
    [[ $f == *.qdf.pdf ]] || files+=("$f") # skip qpdf's decompressed copies
done
if [ ${#files[@]} -eq 0 ]; then
    echo "error: no PDFs in target/test-pdfs; run 'cargo test -p pdfgen-core' first" >&2
    exit 2
fi

echo "$("$PDFCPU" version 2>&1 | head -1)"
failed=0
for f in "${files[@]}"; do
    if out=$("$PDFCPU" validate --mode strict "$f" 2>&1); then
        echo "ok    $f"
    else
        echo "FAIL  $f"
        echo "$out" | sed 's/^/      /'
        failed=$((failed + 1))
    fi
done
echo "${#files[@]} file(s), $failed failure(s)"
[ "$failed" -eq 0 ]
