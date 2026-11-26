#!/usr/bin/env python3
"""Checks that every wrapper declares the same C ABI as ffi/src/lib.rs.

Run from anywhere: `python3 tools/abi-check.py [--root DIR]`. Exits 0 when all six declaration sites
(Rust, C#, Node, Python, Java, Go) agree, 1 on any difference, 2 if a declaration cannot be parsed.

What is compared, per exported function: the name, the argument count, and each argument and the return
value reduced to a width class: ptr, i32 (any 32-bit integer), i64 (any 64-bit integer, i.e. usize), f32,
void. Signedness and pointee types are NOT compared: Java has no unsigned types and several wrappers
pass every pointer as an untyped one. Error-code and font constants are not compared either.

Standard library only.
"""
import ast
import re
import sys
from pathlib import Path


class ParseError(Exception):
    pass


def split_args(s):
    """Splits a parameter list on top-level commas."""
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "([<":
            depth += 1
        elif ch in ")]>":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def classify(table, raw, where):
    raw = " ".join(raw.split())
    for pred, cls in table:
        if pred(raw):
            return cls
    raise ParseError(f"{where}: unknown type {raw!r}")


# --- Rust (source of truth) ------------------------------------------------------------------------------
RUST_SCALARS = {"i32": "i32", "u32": "i32", "usize": "i64", "f32": "f32"}


def rust(root):
    src = (root / "ffi/src/lib.rs").read_text()
    pat = re.compile(r'#\[no_mangle\]\s*pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(\w+)\s*\((.*?)\)\s*(?:->\s*([^{]+?))?\s*\{', re.S)
    fns = {}
    for m in pat.finditer(src):
        name, params, ret = m.group(1), m.group(2), m.group(3)

        def cls(t):
            t = " ".join(t.split())
            if t.startswith("*"):
                return "ptr"
            if t in RUST_SCALARS:
                return RUST_SCALARS[t]
            raise ParseError(f"rust {name}: unknown type {t!r}")

        args = [cls(a.split(":", 1)[1]) for a in split_args(params)]
        fns[name] = (cls(ret) if ret else "void", args)
    total = len(re.findall(r'extern\s+"C"\s+fn', src))
    if total != len(fns):
        raise ParseError(f'rust: {total} extern "C" fns but {len(fns)} are #[no_mangle] exports')
    return fns


# --- C# --------------------------------------------------------------------------------------------------
CS = [
    (lambda t: t == "void", "void"),
    (lambda t: t in ("int", "uint"), "i32"),
    (lambda t: t in ("UIntPtr", "nuint", "ulong", "long"), "i64"),
    (lambda t: t == "float", "f32"),
    (lambda t: t in ("IntPtr", "nint") or t.endswith("[]") or t.startswith("out ") or t.startswith("ref "), "ptr"),
]


def csharp(root):
    src = (root / "dotnet/PdfGen/Native.cs").read_text()
    fns = {}
    for m in re.finditer(r"static\s+extern\s+(\S+)\s+(pdfgen_\w+)\s*\((.*?)\)\s*;", src, re.S):
        ret, name, params = m.groups()
        args = [classify(CS, " ".join(a.split()).rsplit(" ", 1)[0], f"c# {name}") for a in split_args(params)]
        fns[name] = (classify(CS, ret, f"c# {name}"), args)
    return fns


# --- Node (koffi) ----------------------------------------------------------------------------------------
C_TYPES = [
    (lambda t: "*" in t, "ptr"),
    (lambda t: t.split()[-1] == "void", "void"),
    (lambda t: t.split()[-1] in ("int", "int32_t", "uint32_t", "unsigned"), "i32"),
    (lambda t: t.split()[-1] in ("size_t", "uint64_t", "int64_t"), "i64"),
    (lambda t: t.split()[-1] == "float", "f32"),
]


def parse_c_decl(decl, who):
    m = re.match(r"^\s*(.*?)\s*\b(pdfgen_\w+)\s*\((.*)\)\s*$", " ".join(decl.split()), re.S)
    if not m:
        raise ParseError(f"{who}: cannot parse {decl!r}")
    ret, name, params = m.groups()
    args = [] if params.strip() in ("", "void") else [
        classify(C_TYPES, re.sub(r"\b[A-Za-z_]\w*$", "", a).strip() if not a.endswith("*") and " " in a else a, f"{who} {name}")
        for a in split_args(params)
    ]
    return name, (classify(C_TYPES, ret, f"{who} {name}"), args)


def node(root):
    src = (root / "node/src/native.js").read_text()
    return dict(parse_c_decl(m.group(1), "node") for m in re.finditer(r"lib\.func\(\s*'([^']+)'", src))


# --- Go (cgo preamble) -----------------------------------------------------------------------------------
def go(root):
    src = (root / "go/pdfgen.go").read_text()
    pre = src.split('import "C"', 1)[0]
    fns = {}
    for m in re.finditer(r"^((?:const\s+)?\w+\s*\*?\s*pdfgen_\w+\s*\([^;]*?\))\s*;", pre, re.M):
        name, sig = parse_c_decl(m.group(1), "go")
        fns[name] = sig
    return fns


# --- Python (ctypes) -------------------------------------------------------------------------------------
def python(root):
    path = root / "python/pdfgen/_native.py"
    tree = ast.parse(path.read_text())

    def cls(node, name):
        text = ast.unparse(node)
        if text == "None":
            return "void"
        if text in ("C.c_int", "C.c_uint32", "C.c_int32", "C.c_uint"):
            return "i32"
        if text in ("C.c_size_t", "C.c_uint64", "C.c_int64"):
            return "i64"
        if text == "C.c_float":
            return "f32"
        if text in ("_p", "C.c_void_p", "C.c_char_p") or text.startswith("C.POINTER("):
            return "ptr"
        raise ParseError(f"python {name}: unknown ctypes type {text!r}")

    fns = {}
    for n in ast.walk(tree):
        if isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and n.func.id == "_declare":
            name = n.args[0].value
            fns[name] = (cls(n.args[1], name), [cls(a, name) for a in n.args[2:]])
    return fns


# --- Java (Panama) ---------------------------------------------------------------------------------------
JAVA = {"ADDRESS": "ptr", "JAVA_INT": "i32", "JAVA_LONG": "i64", "JAVA_FLOAT": "f32"}


def java(root):
    src = next((root / "java/src/main").rglob("Native.java")).read_text()
    fns = {}
    for m in re.finditer(r'handle\(\s*"(pdfgen_\w+)"\s*,\s*FunctionDescriptor\.(of|ofVoid)\(([^()]*)\)\s*\)', src, re.S):
        name, kind, body = m.groups()
        parts = [p.strip() for p in split_args(body)]
        for p in parts:
            if p not in JAVA:
                raise ParseError(f"java {name}: unknown layout {p!r}")
        if kind == "ofVoid":
            fns[name] = ("void", [JAVA[p] for p in parts])
        else:
            fns[name] = (JAVA[parts[0]], [JAVA[p] for p in parts[1:]])
    return fns


SITES = [("c#", csharp), ("node", node), ("python", python), ("java", java), ("go", go)]


def main(argv):
    root = Path(__file__).resolve().parent.parent
    if "--root" in argv:
        root = Path(argv[argv.index("--root") + 1]).resolve()
    try:
        truth = rust(root)
        found = {label: fn(root) for label, fn in SITES}
    except (ParseError, OSError, StopIteration) as e:
        print(f"abi-check: cannot parse: {e}", file=sys.stderr)
        return 2
    if not truth:
        print("abi-check: found no exported functions in ffi/src/lib.rs", file=sys.stderr)
        return 2

    problems = []
    for label, fns in found.items():
        for name in sorted(truth.keys() - fns.keys()):
            problems.append(f"{label}: missing {name}")
        for name in sorted(fns.keys() - truth.keys()):
            problems.append(f"{label}: declares {name}, which ffi/src/lib.rs does not export")
        for name in sorted(truth.keys() & fns.keys()):
            if truth[name] != fns[name]:
                problems.append(f"{label}: {name} is {fns[name][0]}({', '.join(fns[name][1])}), "
                                f"rust has {truth[name][0]}({', '.join(truth[name][1])})")
    if problems:
        print("abi-check: FAILED")
        print("\n".join("  " + p for p in problems))
        return 1
    print(f"abi-check: ok  {len(truth)} functions agree across rust + {', '.join(found)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
