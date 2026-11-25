// Raw koffi declarations for the pdfgen_ffi C ABI. Use index.js instead.
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import koffi from 'koffi';

const ext = { win32: '.dll', darwin: '.dylib' }[process.platform] ?? '.so';
const prefix = process.platform === 'win32' ? '' : 'lib';

// PDFGEN_LIB overrides the default: the workspace's release build.
const libPath =
  process.env.PDFGEN_LIB ??
  join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'target', 'release', `${prefix}pdfgen_ffi${ext}`);

if (!existsSync(libPath)) {
  throw new Error(`pdfgen native library not found at ${libPath}; run \`cargo build --release -p ffi\` or set PDFGEN_LIB`);
}

const lib = koffi.load(libPath);

export const OK = 0;

export const n = {
  pdfgen_last_error: lib.func('const char *pdfgen_last_error(void)'),
  pdfgen_version: lib.func('const char *pdfgen_version(void)'),

  pdfgen_doc_new: lib.func('void *pdfgen_doc_new(void)'),
  pdfgen_doc_free: lib.func('void pdfgen_doc_free(void *doc)'),
  pdfgen_doc_add_text: lib.func('int pdfgen_doc_add_text(void *doc, const char *text, int font, float size)'),
  pdfgen_doc_add_page_break: lib.func('int pdfgen_doc_add_page_break(void *doc)'),
  pdfgen_doc_add_image_raw: lib.func(
    'int pdfgen_doc_add_image_raw(void *doc, uint32_t width, uint32_t height, uint32_t channels, const uint8_t *data, size_t len, float width_pt)',
  ),
  pdfgen_doc_add_image_jpeg: lib.func(
    'int pdfgen_doc_add_image_jpeg(void *doc, const uint8_t *data, size_t len, float width_pt)',
  ),

  pdfgen_table_new: lib.func('void *pdfgen_table_new(const float *widths, size_t ncols, float font_size)'),
  pdfgen_table_free: lib.func('void pdfgen_table_free(void *table)'),
  pdfgen_table_set_header: lib.func('int pdfgen_table_set_header(void *table, const char **cells, size_t n)'),
  pdfgen_table_add_row: lib.func('int pdfgen_table_add_row(void *table, const char **cells, size_t n)'),
  pdfgen_doc_add_table: lib.func('int pdfgen_doc_add_table(void *doc, const void *table)'),

  pdfgen_doc_render: lib.func('int pdfgen_doc_render(const void *doc, _Out_ void **out_data, _Out_ size_t *out_len)'),
  pdfgen_buffer_free: lib.func('void pdfgen_buffer_free(void *data, size_t len)'),
};

/** Copies `len` bytes of native memory at `ptr` into a Buffer that JS owns. */
export function copyBytes(ptr, len) {
  return Buffer.from(new Uint8Array(koffi.view(ptr, len)));
}
