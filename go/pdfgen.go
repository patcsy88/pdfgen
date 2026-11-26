// Package pdfgen builds PDF reports through the pdfgen C ABI (libpdfgen_ffi) using cgo.
//
// The native library keeps its last error message per OS thread, so every call that can fail runs on a
// locked OS thread and reads the message before unlocking. A Document or Table must not be used from
// several goroutines at once; use one per goroutine.
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

// Table is a table under construction. Close it after adding it to a document.
type Table struct {
	handle  *C.PdfgenTable
	cleanup runtime.Cleanup
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
	t := &Table{handle: h}
	t.cleanup = runtime.AddCleanup(t, func(h *C.PdfgenTable) { C.pdfgen_table_free(h) }, h)
	return t, nil
}

// Close frees the native table. It is safe to call more than once.
func (t *Table) Close() {
	if t.handle != nil {
		t.cleanup.Stop()
		C.pdfgen_table_free(t.handle)
		t.handle = nil
	}
}

func (t *Table) setCells(cells []string, native func(*C.PdfgenTable, **C.char, C.size_t) C.int32_t) error {
	if t.handle == nil {
		return ErrClosed
	}
	arr, free, err := cStrings(cells)
	if err != nil {
		return err
	}
	defer free()
	return call(func() C.int32_t { return native(t.handle, arr, C.size_t(len(cells))) })
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
	handle  *C.PdfgenDoc
	cleanup runtime.Cleanup
}

// NewDocument creates an empty document.
func NewDocument() (*Document, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	h := C.pdfgen_doc_new()
	if h == nil {
		return nil, &Error{Code: errCore, Message: "pdfgen_doc_new returned null"}
	}
	d := &Document{handle: h}
	d.cleanup = runtime.AddCleanup(d, func(h *C.PdfgenDoc) { C.pdfgen_doc_free(h) }, h)
	return d, nil
}

// Close frees the native document. It is safe to call more than once.
func (d *Document) Close() {
	if d.handle != nil {
		d.cleanup.Stop()
		C.pdfgen_doc_free(d.handle)
		d.handle = nil
	}
}

// AddText adds a wrapped paragraph. Text must be valid UTF-8 without NUL characters.
func (d *Document) AddText(text string, font Font, size float32) error {
	if d.handle == nil {
		return ErrClosed
	}
	c, err := cString(text, "text")
	if err != nil {
		return err
	}
	defer C.free(unsafe.Pointer(c))
	return call(func() C.int32_t {
		return C.pdfgen_doc_add_text(d.handle, c, C.int32_t(font), C.float(size))
	})
}

// AddPageBreak starts a new page.
func (d *Document) AddPageBreak() error {
	if d.handle == nil {
		return ErrClosed
	}
	return call(func() C.int32_t { return C.pdfgen_doc_add_page_break(d.handle) })
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
	if d.handle == nil {
		return ErrClosed
	}
	return call(func() C.int32_t {
		return C.pdfgen_doc_add_image_raw(d.handle, C.uint32_t(width), C.uint32_t(height), C.uint32_t(channels),
			bytesPtr(pixels), C.size_t(len(pixels)), C.float(widthPt))
	})
}

// AddJPEG embeds a JPEG file unchanged, drawn widthPt points wide.
func (d *Document) AddJPEG(jpeg []byte, widthPt float32) error {
	if d.handle == nil {
		return ErrClosed
	}
	return call(func() C.int32_t {
		return C.pdfgen_doc_add_image_jpeg(d.handle, bytesPtr(jpeg), C.size_t(len(jpeg)), C.float(widthPt))
	})
}

// AddTable copies the table into the document; the caller still owns t.
func (d *Document) AddTable(t *Table) error {
	if d.handle == nil || t == nil || t.handle == nil {
		return ErrClosed
	}
	return call(func() C.int32_t { return C.pdfgen_doc_add_table(d.handle, t.handle) })
}

// Render lays out the document and returns the PDF bytes.
func (d *Document) Render() ([]byte, error) {
	if d.handle == nil {
		return nil, ErrClosed
	}
	var data *C.uint8_t
	var n C.size_t
	if err := call(func() C.int32_t { return C.pdfgen_doc_render(d.handle, &data, &n) }); err != nil {
		return nil, err
	}
	defer C.pdfgen_buffer_free(data, n)
	return C.GoBytes(unsafe.Pointer(data), C.int(n)), nil
}

// Save renders the document and writes it to path.
func (d *Document) Save(path string) error {
	pdf, err := d.Render()
	if err != nil {
		return err
	}
	return os.WriteFile(path, pdf, 0o644)
}
