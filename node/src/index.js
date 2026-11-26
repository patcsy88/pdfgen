// Node.js wrapper over the pdfgen C ABI, mirroring the .NET wrapper (dotnet/PdfGen).
import { OK, copyBytes, n } from './native.js';

/** The four built-in faces. Values match the C ABI font codes. */
export const Font = Object.freeze({
  Helvetica: 0,
  HelveticaBold: 1,
  Courier: 2,
  CourierBold: 3,
});

/** Raised when the native library reports an error. `code` is the negative PDFGEN_ERR_* value. */
export class PdfError extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'PdfError';
    this.code = code;
  }
}

const ERR_ARG = -3;
const ERR_CORE = -4;

const lastError = () => n.pdfgen_last_error() ?? '';

function check(rc) {
  if (rc !== OK) throw new PdfError(rc, lastError());
}

function requireString(value, name) {
  if (typeof value !== 'string') throw new TypeError(`${name} must be a string`);
  if (value.includes('\0')) throw new TypeError(`${name} must not contain a NUL character`);
  return value;
}

function requireCells(cells) {
  if (!Array.isArray(cells)) throw new TypeError('cells must be an array of strings');
  cells.forEach((c, i) => requireString(c, `cells[${i}]`));
  return cells;
}

function requireNonNegativeInt(value, name) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    throw new RangeError(`${name} must be an integer from 0 to 4294967295`);
  }
  return value;
}

// Frees native objects that were never disposed explicitly.
const docs = new FinalizationRegistry((h) => n.pdfgen_doc_free(h));
const tables = new FinalizationRegistry((h) => n.pdfgen_table_free(h));

/** A table under construction. Call `dispose()` after adding it to a document. */
export class PdfTable {
  #handle;

  /** @param {number[]} columnWidths widths in points @param {number} [fontSize] */
  constructor(columnWidths, fontSize = 10) {
    if (!Array.isArray(columnWidths)) throw new TypeError('columnWidths must be an array of numbers');
    const handle = n.pdfgen_table_new(Float32Array.from(columnWidths), columnWidths.length, fontSize);
    if (handle === null) throw new PdfError(ERR_ARG, lastError());
    this.#handle = handle;
    tables.register(this, handle, this);
  }

  get handle() {
    if (this.#handle === null) throw new Error('PdfTable is disposed');
    return this.#handle;
  }

  setHeader(...cells) {
    check(n.pdfgen_table_set_header(this.handle, requireCells(cells), cells.length));
    return this;
  }

  addRow(...cells) {
    check(n.pdfgen_table_add_row(this.handle, requireCells(cells), cells.length));
    return this;
  }

  dispose() {
    const h = this.#handle;
    if (h === null) return;
    this.#handle = null;
    tables.unregister(this);
    n.pdfgen_table_free(h);
  }

  [Symbol.dispose]() {
    this.dispose();
  }
}

/** A document being built. Call `dispose()` when done. */
export class PdfDocument {
  #handle;

  constructor() {
    const handle = n.pdfgen_doc_new();
    if (handle === null) throw new PdfError(ERR_CORE, 'pdfgen_doc_new returned null');
    this.#handle = handle;
    docs.register(this, handle, this);
  }

  /** Version of the native library. */
  static get nativeVersion() {
    return n.pdfgen_version() ?? '';
  }

  get #h() {
    if (this.#handle === null) throw new Error('PdfDocument is disposed');
    return this.#handle;
  }

  addText(text, font = Font.Helvetica, size = 12) {
    check(n.pdfgen_doc_add_text(this.#h, requireString(text, 'text'), font, size));
    return this;
  }

  addPageBreak() {
    check(n.pdfgen_doc_add_page_break(this.#h));
    return this;
  }

  /** Adds 8-bit RGB (`channels` = 3) or gray (1) pixels, drawn `widthPt` points wide. */
  addRawImage(width, height, channels, pixels, widthPt) {
    requireNonNegativeInt(width, 'width');
    requireNonNegativeInt(height, 'height');
    requireNonNegativeInt(channels, 'channels');
    if (!(pixels instanceof Uint8Array)) throw new TypeError('pixels must be a Buffer or Uint8Array');
    check(n.pdfgen_doc_add_image_raw(this.#h, width, height, channels, pixels, pixels.length, widthPt));
    return this;
  }

  addJpeg(jpeg, widthPt) {
    if (!(jpeg instanceof Uint8Array)) throw new TypeError('jpeg must be a Buffer or Uint8Array');
    check(n.pdfgen_doc_add_image_jpeg(this.#h, jpeg, jpeg.length, widthPt));
    return this;
  }

  /** Copies the table into the document; the caller still owns `table`. */
  addTable(table) {
    if (!(table instanceof PdfTable)) throw new TypeError('table must be a PdfTable');
    check(n.pdfgen_doc_add_table(this.#h, table.handle));
    return this;
  }

  /** Lays out the document and returns the PDF bytes as a Buffer. */
  render() {
    const outData = [null];
    const outLen = [0];
    check(n.pdfgen_doc_render(this.#h, outData, outLen));
    const [data] = outData;
    const [len] = outLen;
    try {
      return copyBytes(data, Number(len));
    } finally {
      n.pdfgen_buffer_free(data, len);
    }
  }

  dispose() {
    const h = this.#handle;
    if (h === null) return;
    this.#handle = null;
    docs.unregister(this);
    n.pdfgen_doc_free(h);
  }

  [Symbol.dispose]() {
    this.dispose();
  }
}
