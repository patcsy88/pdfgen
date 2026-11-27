// Package pdfgen builds PDF reports through the pdfgen C ABI (libpdfgen_ffi) using cgo.
//
// The native library keeps its last error message per OS thread, so every call that can fail runs on a
// locked OS thread and reads the message before unlocking. Each Document and Table serialises its own
// calls, Close included, with a mutex: Close may be called from any goroutine, waits for a call in flight,
// and copying a Document or Table value shares one native handle. Building a document from several
// goroutines at once is memory-safe but leaves the order of the calls undefined; use one per goroutine.
package pdfgen

/*
#cgo linux LDFLAGS: -L${SRCDIR}/../target/release -lpdfgen_ffi -Wl,-rpath,${SRCDIR}/../target/release
#include <stdint.h>
#include <stdlib.h>

// The repository has no generated header; these mirror ffi/src/lib.rs. Pointer parameters are declared
// without const because cgo converts Go pointer types to the non-const C type.
typedef struct PdfgenDoc PdfgenDoc;
typedef struct PdfgenTable PdfgenTable;

const char* pdfgen_last_error(void);
const char* pdfgen_version(void);
PdfgenDoc* pdfgen_doc_new(void);
void pdfgen_doc_free(PdfgenDoc* doc);
int32_t pdfgen_doc_add_text(PdfgenDoc* doc, char* text, int32_t font, float size);
int32_t pdfgen_doc_add_page_break(PdfgenDoc* doc);
int32_t pdfgen_doc_add_image_raw(PdfgenDoc* doc, uint32_t width, uint32_t height, uint32_t channels,
	uint8_t* data, size_t len, float width_pt);
int32_t pdfgen_doc_add_image_jpeg(PdfgenDoc* doc, uint8_t* data, size_t len, float width_pt);
PdfgenTable* pdfgen_table_new(float* widths, size_t ncols, float font_size);
void pdfgen_table_free(PdfgenTable* t);
int32_t pdfgen_table_set_header(PdfgenTable* t, char** cells, size_t n);
int32_t pdfgen_table_add_row(PdfgenTable* t, char** cells, size_t n);
int32_t pdfgen_doc_add_table(PdfgenDoc* doc, PdfgenTable* t);
int32_t pdfgen_doc_render(PdfgenDoc* doc, uint8_t** out_data, size_t* out_len);
void pdfgen_buffer_free(uint8_t* data, size_t len);
*/
import "C"

import (
	"errors"
	"fmt"
	"os"
	"runtime"
	"strings"
	"sync"
	"unsafe"
)

// Font selects one of the four built-in faces. The values match the C ABI font codes.
type Font int32

const (
	Helvetica Font = iota
	HelveticaBold
	Courier
	CourierBold
)

// Error codes reported by the native library.
const (
	errArg  = -3
	errCore = -4
)

// Error is a failure reported by the native library. Code is the negative PDFGEN_ERR_* value.
type Error struct {
	Code    int
	Message string
}

func (e *Error) Error() string { return fmt.Sprintf("pdfgen: %s (code %d)", e.Message, e.Code) }

// ErrClosed is returned when a closed Document or Table is used.
var ErrClosed = errors.New("pdfgen: use of closed object")

// lastError returns the calling thread's native error message. The caller must hold the OS thread locked.
func lastError() string { return C.GoString(C.pdfgen_last_error()) }

// call runs f on a locked OS thread and converts a non-zero status into an *Error.
func call(f func() C.int32_t) error {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	if rc := f(); rc != 0 {
		return &Error{Code: int(rc), Message: lastError()}
	}
	return nil
}

// Version returns the version of the native library.
func Version() string { return C.GoString(C.pdfgen_version()) }

// cString copies s to C memory; the caller frees it. A NUL inside s would silently end the C string.
func cString(s, name string) (*C.char, error) {
	if strings.IndexByte(s, 0) >= 0 {
		return nil, fmt.Errorf("pdfgen: %s must not contain NUL characters", name)
	}
	return C.CString(s), nil
}

// cStrings copies cells to a C array of C strings; free releases everything.
func cStrings(cells []string) (arr **C.char, free func(), err error) {
	n := max(len(cells), 1)
	arr = (**C.char)(C.malloc(C.size_t(n) * C.size_t(unsafe.Sizeof(uintptr(0)))))
	slots := unsafe.Slice(arr, n)
	made := 0
	free = func() {
		for i := range made {
			C.free(unsafe.Pointer(slots[i]))
		}
		C.free(unsafe.Pointer(arr))
	}
	for i, c := range cells {
		p, err := cString(c, fmt.Sprintf("cells[%d]", i))
		if err != nil {
			free()
			return nil, nil, err
		}
		slots[i] = p
		made++
	}
	return arr, free, nil
}

// owned is a native handle shared by every copy of the Go value that wraps it. The mutex makes Close wait
// for a call in flight and makes concurrent Close calls free the handle once. The handle is untyped
// because cgo's incomplete struct types cannot be type arguments.
type owned struct {
	mu      sync.Mutex
	handle  unsafe.Pointer
	free    func(unsafe.Pointer)
	cleanup runtime.Cleanup
}

// newOwned takes ownership of h. free must not reference the returned value, or it would never be
// collected.
func newOwned(h unsafe.Pointer, free func(unsafe.Pointer)) *owned {
	o := &owned{handle: h, free: free}
	o.cleanup = runtime.AddCleanup(o, free, h)
	return o
}

// use runs f with the live handle, or returns ErrClosed.
func (o *owned) use(f func(unsafe.Pointer) error) error {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.handle == nil {
		return ErrClosed
	}
	return f(o.handle)
}

func (o *owned) close() {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.handle != nil {
		o.cleanup.Stop()
		o.free(o.handle)
		o.handle = nil
	}
}

func freeTable(p unsafe.Pointer) { C.pdfgen_table_free((*C.PdfgenTable)(p)) }
func freeDoc(p unsafe.Pointer)   { C.pdfgen_doc_free((*C.PdfgenDoc)(p)) }

// Table is a table under construction. Close it after adding it to a document.
type Table struct {
	s *owned
}

// NewTable creates a table with the given column widths in points and font size.
func NewTable(columnWidths []float32, fontSize float32) (*Table, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	var widths *C.float
	if len(columnWidths) > 0 {
		widths = (*C.float)(unsafe.Pointer(&columnWidths[0]))
	}
	h := C.pdfgen_table_new(widths, C.size_t(len(columnWidths)), C.float(fontSize))
	if h == nil {
		return nil, &Error{Code: errArg, Message: lastError()}
	}
	return &Table{s: newOwned(unsafe.Pointer(h), freeTable)}, nil
}

// Close frees the native table. It is safe to call more than once and from several goroutines.
func (t *Table) Close() {
	if t != nil && t.s != nil {
		t.s.close()
	}
}

func (t *Table) use(f func(*C.PdfgenTable) error) error {
	if t == nil || t.s == nil {
		return ErrClosed
	}
	return t.s.use(func(p unsafe.Pointer) error { return f((*C.PdfgenTable)(p)) })
}

func (t *Table) setCells(cells []string, native func(*C.PdfgenTable, **C.char, C.size_t) C.int32_t) error {
	return t.use(func(h *C.PdfgenTable) error {
		arr, free, err := cStrings(cells)
		if err != nil {
			return err
		}
		defer free()
		return call(func() C.int32_t { return native(h, arr, C.size_t(len(cells))) })
	})
}

// SetHeader sets the header row, repeated on every page the table spans.
func (t *Table) SetHeader(cells ...string) error {
	return t.setCells(cells, func(h *C.PdfgenTable, c **C.char, n C.size_t) C.int32_t {
		return C.pdfgen_table_set_header(h, c, n)
	})
}

// AddRow appends a body row. A row with more cells than columns is only reported by Document.Render.
func (t *Table) AddRow(cells ...string) error {
	return t.setCells(cells, func(h *C.PdfgenTable, c **C.char, n C.size_t) C.int32_t {
		return C.pdfgen_table_add_row(h, c, n)
	})
}

// Document is a document being built. Close it when done.
type Document struct {
	s *owned
}

// NewDocument creates an empty document.
func NewDocument() (*Document, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	h := C.pdfgen_doc_new()
	if h == nil {
		return nil, &Error{Code: errCore, Message: "pdfgen_doc_new returned null"}
	}
	return &Document{s: newOwned(unsafe.Pointer(h), freeDoc)}, nil
}

// Close frees the native document. It is safe to call more than once and from several goroutines.
func (d *Document) Close() {
	if d != nil && d.s != nil {
		d.s.close()
	}
}

func (d *Document) use(f func(*C.PdfgenDoc) error) error {
	if d == nil || d.s == nil {
		return ErrClosed
	}
	return d.s.use(func(p unsafe.Pointer) error { return f((*C.PdfgenDoc)(p)) })
}

// AddText adds a wrapped paragraph. Text must be valid UTF-8 without NUL characters.
func (d *Document) AddText(text string, font Font, size float32) error {
	return d.use(func(h *C.PdfgenDoc) error {
		c, err := cString(text, "text")
		if err != nil {
			return err
		}
		defer C.free(unsafe.Pointer(c))
		return call(func() C.int32_t { return C.pdfgen_doc_add_text(h, c, C.int32_t(font), C.float(size)) })
	})
}

// AddPageBreak starts a new page.
func (d *Document) AddPageBreak() error {
	return d.use(func(h *C.PdfgenDoc) error {
		return call(func() C.int32_t { return C.pdfgen_doc_add_page_break(h) })
	})
}

// bytesPtr returns a pointer to the first byte of b, or nil for an empty slice.
func bytesPtr(b []byte) *C.uint8_t {
	if len(b) == 0 {
		return nil
	}
	return (*C.uint8_t)(unsafe.Pointer(&b[0]))
}

// AddRawImage adds 8-bit RGB (channels 3) or gray (channels 1) pixels, drawn widthPt points wide.
func (d *Document) AddRawImage(width, height, channels uint32, pixels []byte, widthPt float32) error {
	return d.use(func(h *C.PdfgenDoc) error {
		return call(func() C.int32_t {
			return C.pdfgen_doc_add_image_raw(h, C.uint32_t(width), C.uint32_t(height), C.uint32_t(channels),
				bytesPtr(pixels), C.size_t(len(pixels)), C.float(widthPt))
		})
	})
}

// AddJPEG embeds a JPEG file unchanged, drawn widthPt points wide.
func (d *Document) AddJPEG(jpeg []byte, widthPt float32) error {
	return d.use(func(h *C.PdfgenDoc) error {
		return call(func() C.int32_t {
			return C.pdfgen_doc_add_image_jpeg(h, bytesPtr(jpeg), C.size_t(len(jpeg)), C.float(widthPt))
		})
	})
}

// AddTable copies the table into the document; the caller still owns t.
func (d *Document) AddTable(t *Table) error {
	// Lock order is always document, then table; nothing locks them the other way round.
	return d.use(func(dh *C.PdfgenDoc) error {
		return t.use(func(th *C.PdfgenTable) error {
			return call(func() C.int32_t { return C.pdfgen_doc_add_table(dh, th) })
		})
	})
}

// Render lays out the document and returns the PDF bytes.
func (d *Document) Render() ([]byte, error) {
	var pdf []byte
	err := d.use(func(h *C.PdfgenDoc) error {
		var data *C.uint8_t
		var n C.size_t
		if err := call(func() C.int32_t { return C.pdfgen_doc_render(h, &data, &n) }); err != nil {
			return err
		}
		defer C.pdfgen_buffer_free(data, n)
		pdf = C.GoBytes(unsafe.Pointer(data), C.int(n))
		return nil
	})
	if err != nil {
		return nil, err
	}
	return pdf, nil
}

// Save renders the document and writes it to path.
func (d *Document) Save(path string) error {
	pdf, err := d.Render()
	if err != nil {
		return err
	}
	return os.WriteFile(path, pdf, 0o644)
}
