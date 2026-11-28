"""Python wrapper over the pdfgen C ABI (ctypes), mirroring the .NET and Node wrappers."""

from __future__ import annotations

import contextlib
import ctypes as C
import enum
import os
import threading
import weakref
from collections.abc import Iterable

from . import _native as _n

__all__ = ["Font", "PdfDocument", "PdfError", "PdfTable", "native_version"]

_ERR_ARG = -3
_ERR_CORE = -4

_bytes_from_buffer = C.pythonapi.PyBytes_FromStringAndSize
_bytes_from_buffer.restype = C.py_object
_bytes_from_buffer.argtypes = (C.c_void_p, C.c_ssize_t)


class Font(enum.IntEnum):
    """The four built-in faces. Values match the C ABI font codes."""

    HELVETICA = 0
    HELVETICA_BOLD = 1
    COURIER = 2
    COURIER_BOLD = 3


class PdfError(Exception):
    """Raised when the native library reports an error. `code` is the negative PDFGEN_ERR_* value."""

    def __init__(self, code: int, message: str) -> None:
        super().__init__(message)
        self.code = code


def native_version() -> str:
    """Version of the native library."""
    return (_n.version() or b"").decode("utf-8", "replace")


def _check(rc: int) -> None:
    if rc != _n.OK:
        raise PdfError(rc, _n.error_message())


def _utf8(value: str, name: str) -> bytes:
    if not isinstance(value, str):
        raise TypeError(f"{name} must be str, not {type(value).__name__}")
    if "\0" in value:
        # ctypes would silently cut the string at the NUL.
        raise ValueError(f"{name} must not contain NUL characters")
    return value.encode("utf-8")


def _cells(cells: Iterable[str]) -> tuple[C.Array, int]:
    encoded = [_utf8(c, f"cells[{i}]") for i, c in enumerate(cells)]
    return (C.c_char_p * len(encoded))(*encoded), len(encoded)


def _uint32(value: int, name: str) -> int:
    # ctypes wraps out-of-range integers instead of rejecting them.
    if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= 0xFFFFFFFF:
        raise ValueError(f"{name} must be an integer from 0 to 4294967295")
    return value


def _buffer(data: bytes | bytearray | memoryview, name: str) -> bytes:
    if type(data) is bytes:
        return data
    if isinstance(data, (bytes, bytearray, memoryview)):
        # Read through the buffer protocol: a bytes subclass can override __len__ (and __bytes__), and the
        # native side is handed the length of this value together with a pointer to its real storage.
        return bytes(memoryview(data))
    raise TypeError(f"{name} must be bytes, bytearray or memoryview, not {type(data).__name__}")


class _Handle:
    """Owns one native handle; the finalizer frees it if `close()` is never called.

    Every native call runs inside `_use()`, which holds the handle's lock, so `close()` from another
    thread waits for an in-flight call instead of freeing the handle under it. Copying is refused: a copy
    would share the raw handle, and closing either one would free memory the other still uses.
    """

    _free = None  # set by subclasses: the matching *_free function
    _what = ""

    def __init__(self, handle: int) -> None:
        self._handle: int | None = handle
        self._lock = threading.Lock()
        self._finalizer = weakref.finalize(self, type(self)._free, handle)
        # Not at interpreter exit: that callback frees without the lock, while a daemon thread can still be
        # inside a native call on this handle. The OS reclaims the memory at exit anyway.
        self._finalizer.atexit = False

    @contextlib.contextmanager
    def _use(self):
        """Yields the live handle with the lock held; raises ValueError once closed."""
        with self._lock:
            if self._handle is None:
                raise ValueError(f"{self._what} is closed")
            yield self._handle

    def close(self) -> None:
        """Frees the native object. Safe to call more than once, from any thread."""
        with self._lock:
            self._handle = None
            self._finalizer()

    def __copy__(self):
        raise TypeError(f"{self._what} owns a native handle and cannot be copied")

    def __deepcopy__(self, memo):
        raise TypeError(f"{self._what} owns a native handle and cannot be copied")

    def __reduce__(self):
        raise TypeError(f"{self._what} owns a native handle and cannot be pickled")

    def __enter__(self):
        return self

    def __exit__(self, *exc) -> None:
        self.close()


class PdfTable(_Handle):
    """A table under construction. Close it after adding it to a document."""

    _free = staticmethod(_n.table_free)
    _what = "PdfTable"

    def __init__(self, column_widths: Iterable[float], font_size: float = 10.0) -> None:
        widths = [float(w) for w in column_widths]
        arr = (C.c_float * len(widths))(*widths)
        handle = _n.table_new(arr, len(widths), font_size)
        if handle is None:
            raise PdfError(_ERR_ARG, _n.error_message())
        super().__init__(handle)

    def set_header(self, *cells: str) -> PdfTable:
        arr, n = _cells(cells)
        with self._use() as h:
            _check(_n.table_set_header(h, arr, n))
        return self

    def add_row(self, *cells: str) -> PdfTable:
        arr, n = _cells(cells)
        with self._use() as h:
            _check(_n.table_add_row(h, arr, n))
        return self


class PdfDocument(_Handle):
    """A document being built. Not thread-safe; use one instance per thread."""

    _free = staticmethod(_n.doc_free)
    _what = "PdfDocument"

    def __init__(self) -> None:
        handle = _n.doc_new()
        if handle is None:
            raise PdfError(_ERR_CORE, "pdfgen_doc_new returned null")
        super().__init__(handle)

    def add_text(self, text: str, font: int = Font.HELVETICA, size: float = 12.0) -> PdfDocument:
        code = int(font)
        if not -(2**31) <= code < 2**31:
            raise ValueError("font code does not fit in a C int")
        with self._use() as h:
            _check(_n.doc_add_text(h, _utf8(text, "text"), code, size))
        return self

    def add_page_break(self) -> PdfDocument:
        with self._use() as h:
            _check(_n.doc_add_page_break(h))
        return self

    def add_raw_image(
        self, width: int, height: int, channels: int, pixels: bytes | bytearray | memoryview, width_pt: float
    ) -> PdfDocument:
        """Adds 8-bit RGB (`channels` = 3) or gray (1) pixels, drawn `width_pt` points wide."""
        data = _buffer(pixels, "pixels")
        with self._use() as h:
            _check(
                _n.doc_add_image_raw(
                    h,
                    _uint32(width, "width"),
                    _uint32(height, "height"),
                    _uint32(channels, "channels"),
                    data,
                    len(data),
                    width_pt,
                )
            )
        return self

    def add_jpeg(self, jpeg: bytes | bytearray | memoryview, width_pt: float) -> PdfDocument:
        data = _buffer(jpeg, "jpeg")
        with self._use() as h:
            _check(_n.doc_add_image_jpeg(h, data, len(data), width_pt))
        return self

    def add_table(self, table: PdfTable) -> PdfDocument:
        """Copies the table into the document; the caller still owns `table`."""
        if not isinstance(table, PdfTable):
            raise TypeError(f"table must be PdfTable, not {type(table).__name__}")
        # Lock order is always document, then table; nothing locks them the other way round.
        with self._use() as h, table._use() as th:
            _check(_n.doc_add_table(h, th))
        return self

    def render(self) -> bytes:
        """Lays out the document and returns the PDF bytes."""
        data = C.c_void_p()
        length = C.c_size_t()
        with self._use() as h:
            _check(_n.doc_render(h, C.byref(data), C.byref(length)))
        try:
            # `C.c_ubyte * n` would build and cache, for good, a new array type for every distinct PDF size, and
            # string_at takes a C int size that cuts anything over 2 GiB; PyBytes_FromStringAndSize has neither.
            return _bytes_from_buffer(data.value, length.value)
        finally:
            _n.buffer_free(data, length.value)

    def save(self, path: str | os.PathLike) -> None:
        """Renders the document and writes it to `path`. A failed render leaves an existing file untouched."""
        pdf = self.render()
        with open(path, "wb") as f:
            f.write(pdf)
