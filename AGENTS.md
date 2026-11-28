# AGENTS.md

Guidance for AI agents working in this repository. Human docs are in `README.md`.

## Project

pdfgen: a small PDF 2.0 report generator. A Rust core (`core/`) sits behind a hand-written C ABI (`ffi/`) with wrappers for .NET, Node.js, Python, Java and Go. Version 0.1.1. Nothing is published to a package registry.

## Commands

Run from the repo root. Wrapper tests need `cargo build --release -p ffi` first, and `qpdf` on PATH.

| What | Command |
|---|---|
| Rust tests | `cargo test --workspace --locked` |
| Rust licenses, advisories, bans, sources | `cargo deny check` |
| C ABI sync, all 7 sites | `python3 tools/abi-check.py` |
| Version sync, all 8 sites | `python3 tools/version-check.py` |
| Font data is current | `python3 tools/gen_fontdata.py --check` |
| Node | `cd node && npm ci && npm test` |
| Python | `cd python && python3 -W error -m unittest discover -s tests` |
| Java | `cd java && mvn -B test` |
| Go | `cd go && LD_LIBRARY_PATH=../target/release go test -race -count=1 ./...` |
| .NET | `cd dotnet && dotnet test` |

Run only the check that covers your change, not every one of them.

## Agents

Pick the least-capable agent that can do the job.

### scout
- **Role:** find where things live and how they connect before an edit.
- **Skills:** repo mapping, tracing a symbol through the wrappers, summarising CI logs.
- **Can:** read, search, run read-only commands.
- **Cannot:** edit files, commit, push.

### task (default)
- **Role:** implement a change end to end and verify it.
- **Can:** edit, build, run the checks above, make a scoped fix.
- **Cannot:** commit, push, tag or release unless the user said so.
- **Must:** give it one area (see ownership below) and the check that proves it.

### reviewer
- **Role:** review a diff for correctness, regressions and missing tests.
- **Use for:** every change that touches `ffi/`, `core/src/`, or more than one wrapper.
- **Cannot:** be the author of the change it reviews.

### security-reviewer
- **Role:** look for vulnerabilities with evidence (a code path or a repro), not guesses.
- **Use for:** `ffi/src/lib.rs` (raw pointers, buffer lengths, `unsafe`), image and JPEG parsing, dependency alerts.
- **Cannot:** edit, or report a finding without a file and line.

### sonic
- **Role:** mechanical work only: renames, version-string bumps, collecting data.
- **Cannot:** design, decide, or touch `ffi/src/lib.rs`, `core/src/lib.rs` or CI.

## Ownership

| Area | Files | Rule |
|---|---|---|
| Core | `core/src/lib.rs` | Public API changes need every wrapper updated in the same change. |
| Font data | `core/src/fontdata.rs` | Generated. Edit `core/afm/` or `tools/gen_fontdata.py`, then regenerate. |
| C ABI | `ffi/src/lib.rs`, `ffi/include/pdfgen.h` | See ABI rules. |
| Wrappers | `dotnet/`, `node/`, `python/`, `java/`, `go/` | Each declares the ABI itself. |
| CI | `.github/workflows/ci.yml` | Actions are pinned to commit SHAs; keep the version comment. |

## Rules

**ABI.** Adding or changing an exported function means editing all 7 sites: `ffi/src/lib.rs`, `pdfgen.h`, `dotnet/PdfGen/Native.cs`, `node/src/native.js`, `python/pdfgen/_native.py`, the Java `Native.java`, and the cgo comment in `go/pdfgen.go`. Then run `tools/abi-check.py`. Panics must never cross the boundary; keep every export inside `catch_unwind`.

**Versions.** The source is `[workspace.package]` in `Cargo.toml`. A bump touches 8 more sites; `tools/version-check.py` lists any that differ.

**Lockfiles.** Commit a manifest change and its lockfile together. Splitting them breaks `--locked` and `npm ci`.

**Licenses.** New dependencies must pass `deny.toml` (Rust) and the npm allowlists. Wrappers have no third-party runtime dependencies; keep it that way.

**Tests that write PDFs.** The PDFs go to each wrapper's `test-output/`, and `external_pdfs` then validates them. Scratch files must end in `.qdf`, `.pdf` or `.out`: qpdf 12 under AppArmor refuses other extensions with "Permission denied".

**Tags.** `vX.Y.Z` and `go/vX.Y.Z` go on the same commit. The Go proxy caches a tag permanently, so never move or reuse one.

## Never

- Commit, push, tag or release without being asked.
- Use `git add`, `git commit -a`, `git stash` or `git reset` in a shared worktree. Use the `commit` helper.
- Edit generated files by hand (`core/src/fontdata.rs`, lockfiles).
- Change `core/tests/arlington/` content; it is vendored with its own license and `SOURCE.txt`.
- Claim a check passed without running it after the last edit.
