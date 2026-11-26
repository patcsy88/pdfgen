/* Built twice by ffi/tests/c_header.rs: as C99 (this file, via cc) and as C++17 (smoke.cpp includes it).
 * Usage: smoke OUT.pdf. Exits 0 on success; prints the failing check and exits 1 otherwise. */
#include <stdio.h>
#include <string.h>

#include "pdfgen.h"

#define CHECK(cond)                                                              \
    do {                                                                         \
        if (!(cond)) {                                                           \
            fprintf(stderr, "%s:%d: check failed: %s\n  last error: %s\n", __FILE__, __LINE__, #cond, \
                    pdfgen_last_error());                                        \
            return 1;                                                            \
        }                                                                        \
    } while (0)

/* The header's constants must match the values the library returns. */
typedef char codes_match[(PDFGEN_OK == 0 && PDFGEN_ERR_NULL == -1 && PDFGEN_ERR_UTF8 == -2 &&
                          PDFGEN_ERR_ARG == -3 && PDFGEN_ERR_CORE == -4 && PDFGEN_ERR_PANIC == -5)
                             ? 1
                             : -1];

int smoke_main(int argc, char **argv) {
    CHECK(argc == 2);
    CHECK(strchr(pdfgen_version(), '.') != NULL);
    CHECK(pdfgen_last_error()[0] == '\0');

    PdfgenDoc *doc = pdfgen_doc_new();
    CHECK(doc != NULL);
    CHECK(pdfgen_doc_add_text(doc, "Title", PDFGEN_FONT_HELVETICA_BOLD, 20.0f) == PDFGEN_OK);
    CHECK(pdfgen_doc_add_text(doc, "caf\xc3\xa9 mono", PDFGEN_FONT_COURIER, 10.0f) == PDFGEN_OK);

    const float widths[2] = {120.0f, 80.0f};
    PdfgenTable *t = pdfgen_table_new(widths, 2, 10.0f);
    CHECK(t != NULL);
    const char *header[2] = {"Name", "Qty"};
    const char *row[2] = {"Widget", "42"};
    CHECK(pdfgen_table_set_header(t, header, 2) == PDFGEN_OK);
    CHECK(pdfgen_table_add_row(t, row, 2) == PDFGEN_OK);
    CHECK(pdfgen_doc_add_table(doc, t) == PDFGEN_OK);
    pdfgen_table_free(t);

    const uint8_t px[2 * 2 * 3] = {255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255};
    CHECK(pdfgen_doc_add_image_raw(doc, 2, 2, 3, px, sizeof px, 40.0f) == PDFGEN_OK);
    CHECK(pdfgen_doc_add_page_break(doc) == PDFGEN_OK);
    CHECK(pdfgen_doc_add_text(doc, "second page", PDFGEN_FONT_HELVETICA, 12.0f) == PDFGEN_OK);

    uint8_t *data = NULL;
    size_t len = 0;
    CHECK(pdfgen_doc_render(doc, &data, &len) == PDFGEN_OK);
    CHECK(data != NULL && len > 8 && memcmp(data, "%PDF-", 5) == 0);

    FILE *f = fopen(argv[1], "wb");
    CHECK(f != NULL);
    CHECK(fwrite(data, 1, len, f) == len);
    CHECK(fclose(f) == 0);

    /* The document survives rendering: add more and render again; the second buffer is larger. */
    CHECK(pdfgen_doc_add_text(doc, "added after render", PDFGEN_FONT_COURIER_BOLD, 12.0f) == PDFGEN_OK);
    uint8_t *data2 = NULL;
    size_t len2 = 0;
    CHECK(pdfgen_doc_render(doc, &data2, &len2) == PDFGEN_OK);
    CHECK(len2 > len);
    pdfgen_buffer_free(data, len);
    pdfgen_buffer_free(data2, len2);

    /* Errors are codes plus a message. */
    CHECK(pdfgen_doc_add_text(NULL, "x", 0, 12.0f) == PDFGEN_ERR_NULL);
    CHECK(strcmp(pdfgen_last_error(), "null doc") == 0);
    CHECK(pdfgen_doc_add_text(doc, "x", 9, 12.0f) == PDFGEN_ERR_ARG);
    CHECK(pdfgen_doc_add_text(doc, "\xff", 0, 12.0f) == PDFGEN_ERR_UTF8);
    CHECK(pdfgen_doc_add_image_raw(doc, 2, 2, 3, px, 5, 40.0f) == PDFGEN_ERR_CORE);
    CHECK(pdfgen_table_new(NULL, 0, 10.0f) == NULL);
    CHECK(pdfgen_last_error()[0] != '\0');

    /* A row with too many cells is accepted by the table and rejected at render; no buffer is returned. */
    const float one[1] = {100.0f};
    PdfgenTable *narrow = pdfgen_table_new(one, 1, 10.0f);
    CHECK(narrow != NULL);
    CHECK(pdfgen_table_add_row(narrow, row, 2) == PDFGEN_OK);
    PdfgenDoc *bad = pdfgen_doc_new();
    CHECK(bad != NULL);
    CHECK(pdfgen_doc_add_table(bad, narrow) == PDFGEN_OK);
    uint8_t *none = (uint8_t *)1;
    size_t none_len = 7;
    CHECK(pdfgen_doc_render(bad, &none, &none_len) == PDFGEN_ERR_CORE);
    CHECK(none == NULL && none_len == 0);
    pdfgen_table_free(narrow);
    pdfgen_doc_free(bad);

    pdfgen_doc_free(doc);
    pdfgen_doc_free(NULL);
    pdfgen_table_free(NULL);
    pdfgen_buffer_free(NULL, 0);
    return 0;
}

#ifndef SMOKE_NO_MAIN
int main(int argc, char **argv) { return smoke_main(argc, argv); }
#endif
