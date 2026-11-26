"""Raw ctypes declarations for the pdfgen_ffi C ABI. Use the names exported by `pdfgen` instead."""

import ctypes as C
import os
import sys
from pathlib import Path

OK = 0

_EXT = {"win32": ".dll", "darwin": ".dylib"}.get(sys.platform, ".so")
_PREFIX = "" if sys.platform == "win32" else "lib"


def _library_path() -> Path:
    # PDFGEN_LIB overrides the default: the workspace's release build.
    override = os.environ.get("PDFGEN_LIB")
    if override:
        return Path(override)
    repo = Path(__file__).resolve().parents[2]
    return repo / "target" / "release" / f"{_PREFIX}pdfgen_ffi{_EXT}"


def _load() -> C.CDLL:
    path = _library_path()
    if not path.exists():
        raise OSError(
            f"pdfgen native library not found at {path}; "
            "run `cargo build --release -p ffi` or set PDFGEN_LIB"
        )
    return C.CDLL(str(path))


lib = _load()


def _declare(name: str, restype, *argtypes):
    fn = getattr(lib, name)
    fn.restype = restype
    fn.argtypes = list(argtypes)
    return fn


_p = C.c_void_p

last_error = _declare("pdfgen_last_error", C.c_char_p)
version = _declare("pdfgen_version", C.c_char_p)

doc_new = _declare("pdfgen_doc_new", _p)
doc_free = _declare("pdfgen_doc_free", None, _p)
doc_add_text = _declare("pdfgen_doc_add_text", C.c_int, _p, C.c_char_p, C.c_int, C.c_float)
doc_add_page_break = _declare("pdfgen_doc_add_page_break", C.c_int, _p)
doc_add_image_raw = _declare(
    "pdfgen_doc_add_image_raw", C.c_int, _p, C.c_uint32, C.c_uint32, C.c_uint32, _p, C.c_size_t, C.c_float
)
doc_add_image_jpeg = _declare("pdfgen_doc_add_image_jpeg", C.c_int, _p, _p, C.c_size_t, C.c_float)

table_new = _declare("pdfgen_table_new", _p, _p, C.c_size_t, C.c_float)
table_free = _declare("pdfgen_table_free", None, _p)
table_set_header = _declare("pdfgen_table_set_header", C.c_int, _p, _p, C.c_size_t)
table_add_row = _declare("pdfgen_table_add_row", C.c_int, _p, _p, C.c_size_t)
doc_add_table = _declare("pdfgen_doc_add_table", C.c_int, _p, _p)

doc_render = _declare(
    "pdfgen_doc_render", C.c_int, _p, C.POINTER(C.c_void_p), C.POINTER(C.c_size_t)
)
buffer_free = _declare("pdfgen_buffer_free", None, _p, C.c_size_t)


def error_message() -> str:
    """The calling thread's last native error message."""
    return (last_error() or b"").decode("utf-8", "replace")
