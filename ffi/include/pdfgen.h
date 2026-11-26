/*
 * pdfgen.h - C API of libpdfgen_ffi (Rust crate `ffi`, source: ffi/src/lib.rs).
 *
 * Hand-written, not generated. tools/abi-check.py compares every declaration and error code below with
 * ffi/src/lib.rs, and ffi/tests/c_header.rs compiles this file as C99 and as C++17 and runs a program
 * against the built library.
 *
 * Conventions
 *  - Handles are opaque. Free each with its matching *_free function; passing NULL to a *_free
 *    function does nothing.
 *  - Functions returning int32_t return PDFGEN_OK (0) on success and a negative PDFGEN_ERR_* code on
 *    failure. The message for the failure is available from pdfgen_last_error().
 *  - Strings are NUL-terminated UTF-8.
 *  - A document or table must not be used from several threads at once. The error message is kept per
 *    thread, so call pdfgen_last_error() on the thread that made the failing call.
 *  - Panics never cross this boundary; they are reported as PDFGEN_ERR_PANIC.
 *
 * Not tested: Windows and macOS. Nothing here is platform specific, but the library has only been
 * built and run on Linux.
 */
#ifndef PDFGEN_H
#define PDFGEN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes. */
#define PDFGEN_OK 0
#define PDFGEN_ERR_NULL (-1)  /* a required pointer argument was NULL */
#define PDFGEN_ERR_UTF8 (-2)  /* a string argument is not valid UTF-8 */
#define PDFGEN_ERR_ARG (-3)   /* an argument is out of range, e.g. a bad font code or non-positive size */
#define PDFGEN_ERR_CORE (-4)  /* the PDF engine rejected the input; see pdfgen_last_error() */
#define PDFGEN_ERR_PANIC (-5) /* internal panic, caught at the boundary */

/* Font codes for pdfgen_doc_add_text(): the four built-in PDF base fonts (WinAnsi encoding). */
#define PDFGEN_FONT_HELVETICA 0
#define PDFGEN_FONT_HELVETICA_BOLD 1
#define PDFGEN_FONT_COURIER 2
#define PDFGEN_FONT_COURIER_BOLD 3

typedef struct PdfgenDoc PdfgenDoc;     /* a document being built */
typedef struct PdfgenTable PdfgenTable; /* a table being built */

/* Message for the last failure on the calling thread. Never NULL; empty if nothing has failed on this
 * thread. Do not free. Valid until the next pdfgen call on the same thread. */
const char *pdfgen_last_error(void);

/* Library version, e.g. "0.1.0". Static; do not free. */
const char *pdfgen_version(void);

/* Creates an empty document. Release it with pdfgen_doc_free(). */
PdfgenDoc *pdfgen_doc_new(void);

/* Frees a document. doc may be NULL. */
void pdfgen_doc_free(PdfgenDoc *doc);

/* Adds wrapped text. font is a PDFGEN_FONT_* code; size is in points and must be finite and > 0.
 * A size above (842 - 2 * 50) / 1.2 = 618.33 (one line as tall as the page's text area) is accepted here
 * but makes pdfgen_doc_render() fail with PDFGEN_ERR_CORE.
 * Errors: NULL doc or text (PDFGEN_ERR_NULL), invalid UTF-8 (PDFGEN_ERR_UTF8), bad font or size
 * (PDFGEN_ERR_ARG). */
int32_t pdfgen_doc_add_text(PdfgenDoc *doc, const char *text, int32_t font, float size);

/* Starts a new page. */
int32_t pdfgen_doc_add_page_break(PdfgenDoc *doc);

/* Adds an image from raw 8-bit pixels. channels is 3 (RGB) or 1 (gray); len must equal
 * width * height * channels, otherwise PDFGEN_ERR_CORE. width_pt is the drawn width in points and must
 * be finite and > 0; the height follows the aspect ratio. The data is copied. data may be NULL only if
 * len is 0. */
int32_t pdfgen_doc_add_image_raw(PdfgenDoc *doc, uint32_t width, uint32_t height, uint32_t channels,
                                 const uint8_t *data, size_t len, float width_pt);

/* Adds a JPEG, embedded without re-encoding. A file that is not a JPEG the engine can read gives
 * PDFGEN_ERR_CORE. The data is copied. */
int32_t pdfgen_doc_add_image_jpeg(PdfgenDoc *doc, const uint8_t *data, size_t len, float width_pt);

/* Creates a table with ncols columns of the given widths in points, and the given font size.
 * Returns NULL on error (message in pdfgen_last_error()): widths NULL, ncols 0, or any width or the font
 * size not finite and > 0. widths is copied. Release with pdfgen_table_free(). */
PdfgenTable *pdfgen_table_new(const float *widths, size_t ncols, float font_size);

/* Frees a table. t may be NULL. */
void pdfgen_table_free(PdfgenTable *t);

/* Sets the header row from n NUL-terminated UTF-8 strings (copied). */
int32_t pdfgen_table_set_header(PdfgenTable *t, const char *const *cells, size_t n);

/* Appends a row of n NUL-terminated UTF-8 strings (copied). A row with more cells than the table has
 * columns is not rejected here: pdfgen_doc_render() fails with PDFGEN_ERR_CORE. */
int32_t pdfgen_table_add_row(PdfgenTable *t, const char *const *cells, size_t n);

/* Copies the table's current state into the document. The caller still owns t and must free it; later
 * changes to t do not affect the document. */
int32_t pdfgen_doc_add_table(PdfgenDoc *doc, const PdfgenTable *t);

/* Renders the document to PDF bytes. On success *out_data points to *out_len bytes, to be released with
 * pdfgen_buffer_free(*out_data, *out_len). On failure *out_data is NULL and *out_len is 0. Besides the
 * row-width case above, it fails with PDFGEN_ERR_CORE when a text size is above 618.33, a table font
 * size is above 1000, a column width is at most 8 or above 32767, the column widths add up to more than
 * 32717, or a table row (together with the header) is taller than a page. The document is not consumed:
 * it can be changed and rendered again. */
int32_t pdfgen_doc_render(const PdfgenDoc *doc, uint8_t **out_data, size_t *out_len);

/* Frees a buffer from pdfgen_doc_render(). data and len must be exactly what that call returned; free
 * each buffer once. data may be NULL. */
void pdfgen_buffer_free(uint8_t *data, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* PDFGEN_H */
