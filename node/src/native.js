// Raw koffi declarations for the pdfgen_ffi C ABI. Use index.js instead.
import { existsSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import koffi from 'koffi';

const ext = { win32: '.dll', darwin: '.dylib' }[process.platform] ?? '.so';
const prefix = process.platform === 'win32' ? '' : 'lib';

// PDFGEN_LIB overrides the default. Made absolute so the existence check and the load see the same file:
// a bare name would otherwise be handed to dlopen, which searches the loader path instead of the cwd.
// The default is the workspace's release build, valid only in a source checkout, which has the layout
// <repo>/node/src/native.js next to <repo>/ffi. Any other copy (installed, vendored) has some other directory
// two levels up, shared with other packages (node_modules) or workspaces; loading code from there would run
// whatever file sits at that path. The directory names are checked as well as ffi/Cargo.toml, so a copy that
// merely sits two levels below an unrelated Rust workspace does not match.
function libraryPath() {
  if (process.env.PDFGEN_LIB) return resolve(process.env.PDFGEN_LIB);
  const src = dirname(fileURLToPath(import.meta.url));
  const pkg = dirname(src);
  const repo = dirname(pkg);
  const inCheckout = basename(src) === 'src' && basename(pkg) === 'node' && existsSync(join(repo, 'ffi', 'Cargo.toml'));
  if (!inCheckout) {
    throw new Error(
      `pdfgen native library location is unknown: this copy of pdfgen is not in a source checkout; set PDFGEN_LIB to the path of ${prefix}pdfgen_ffi${ext}`,
    );
  }
  return join(repo, 'target', 'release', `${prefix}pdfgen_ffi${ext}`);
}

const libPath = libraryPath();

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
