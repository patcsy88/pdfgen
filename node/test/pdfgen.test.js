// Tests for the koffi wrapper. Every PDF is written to node/test-output/ and must pass
// `qpdf --check`; the Arlington and content-stream checks then run over the whole
// directory via `PDFGEN_CHECK_DIR=node/test-output cargo test -p pdfgen-core --test external_pdfs`.
// qpdf is not a PDF 2.0 conformance validator; see README.md for what the checks cover.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { before, describe, it } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Font, PdfDocument, PdfError, PdfTable } from '../src/index.js';

const outDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'test-output');

before(() => {
  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
});

function qpdf(...args) {
  const r = spawnSync('qpdf', args, { encoding: 'utf8' });
  if (r.error) throw new Error('qpdf must be installed to run these tests (apt install qpdf)', { cause: r.error });
  return { status: r.status, output: r.stdout + r.stderr };
}

/** Writes the PDF to test-output/, requires `qpdf --check` to exit 0, returns the path. */
function checked(name, pdf) {
  const path = join(outDir, name);
  writeFileSync(path, pdf);
  const r = qpdf('--check', path);
  assert.equal(r.status, 0, `qpdf --check exited ${r.status} for ${name}:\n${r.output}`);
  return path;
}

function pageCount(path) {
  const r = qpdf('--show-npages', path);
  assert.equal(r.status, 0, r.output);
  return Number(r.output.trim());
}

/** The PDF with all streams and object streams decompressed, as text. */
function qdf(path) {
  const out = `${path}.qdf`;
  const r = qpdf('--qdf', '--object-streams=disable', path, out);
  assert.ok(r.status === 0 || r.status === 3, r.output);
  const text = readFileSync(out, 'latin1');
  rmSync(out);
  return text;
}

function gradient(w, h, channels) {
  const px = Buffer.alloc(w * h * channels);
  for (let i = 0; i < px.length; i++) px[i] = (i * 7) & 0xff;
  return px;
}

/**
 * Minimal valid baseline grayscale JPEG (flat mid-gray); `w` and `h` multiples of 8.
 * One 8x8 block is a DC code "0" plus an EOB code "0", each a 1-bit Huffman code.
 * Port of `gray_jpeg` in core/tests/pdf.rs, so qpdf can decode the DCT stream.
 */
function grayJpeg(w, h) {
  const be16 = (v) => [v >> 8, v & 0xff];
  const blocks = (w / 8) * (h / 8);
  const bits = blocks * 2;
  const data = Buffer.alloc(Math.ceil(bits / 8));
  const pad = data.length * 8 - bits;
  if (pad > 0) data[data.length - 1] |= (1 << pad) - 1;
  return Buffer.concat([
    Buffer.from([0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00]),
    Buffer.alloc(64, 1),
    Buffer.from([0xff, 0xc0, 0x00, 0x0b, 0x08, ...be16(h), ...be16(w), 0x01, 0x01, 0x11, 0x00]),
    ...[0x00, 0x10].map((classId) =>
      Buffer.concat([Buffer.from([0xff, 0xc4, 0x00, 0x14, classId, 0x01]), Buffer.alloc(15), Buffer.from([0x00])]),
    ),
    Buffer.from([0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]),
    data,
    Buffer.from([0xff, 0xd9]),
  ]);
}

function render(build) {
  const doc = new PdfDocument();
  try {
    build(doc);
    return doc.render();
  } finally {
    doc.dispose();
  }
}

describe('text', () => {
  it('reports the native version', () => {
    assert.match(PdfDocument.nativeVersion, /^\d+\.\d+\.\d+/);
  });

  it('draws text, including non-ASCII Latin-1, as PDF 2.0', () => {
    const pdf = render((d) => d.addText('Hello from Node, café', Font.Helvetica, 14));
    assert.equal(pdf.subarray(0, 8).toString('latin1'), '%PDF-2.0');
    const text = qdf(checked('text.pdf', pdf));
    assert.match(text, /\/F1 14 Tf/);
    assert.equal(pageCount(join(outDir, 'text.pdf')), 1);
  });

  it('wraps long text onto further pages', () => {
    const pdf = render((d) => {
      for (let i = 0; i < 120; i++) d.addText(`Line ${i}: the quick brown fox jumps over the lazy dog.`, Font.Helvetica, 12);
    });
    assert.ok(pageCount(checked('flow.pdf', pdf)) >= 2);
  });

  it('is byte-identical for identical input', () => {
    const make = () => render((d) => d.addText('same', Font.Courier, 10));
    assert.deepEqual(make(), make());
  });

  it('renders an empty document as valid PDF', () => {
    assert.equal(pageCount(checked('empty.pdf', render(() => {}))), 1);
  });
});

describe('fonts', () => {
  it('declares and uses all four built-in faces', () => {
    const pdf = render((d) => {
      d.addText('sans', Font.Helvetica, 12)
        .addText('sans bold', Font.HelveticaBold, 12)
        .addText('mono', Font.Courier, 12)
        .addText('mono bold', Font.CourierBold, 12);
    });
    const text = qdf(checked('fonts.pdf', pdf));
    for (const base of ['Helvetica', 'Helvetica-Bold', 'Courier', 'Courier-Bold']) {
      assert.ok(text.includes(`/BaseFont /${base}`), `missing /BaseFont /${base}`);
    }
    for (const f of ['F1', 'F2', 'F3', 'F4']) assert.match(text, new RegExp(`/${f} 12 Tf`));
  });

  it('rejects an unknown font code and a bad size with the native message', () => {
    const doc = new PdfDocument();
    try {
      assert.throws(() => doc.addText('x', 9, 12), (e) => e instanceof PdfError && e.code === -3 && /font/.test(e.message));
      assert.throws(() => doc.addText('x', Font.Helvetica, 0), (e) => e instanceof PdfError && e.code === -3);
      assert.throws(() => doc.addText('x', Font.Helvetica, Number.NaN), (e) => e instanceof PdfError && e.code === -3);
    } finally {
      doc.dispose();
    }
  });
});

describe('tables', () => {
  const sample = () =>
    new PdfTable([120, 80, 80], 10).setHeader('Item', 'Qty', 'Price').addRow('Widget', '4', '3.50').addRow('Gadget', '12', '19.99');

  it('renders header, cells and borders', () => {
    const pdf = render((d) => {
      const t = sample();
      d.addTable(t);
      t.dispose();
    });
    const text = qdf(checked('table.pdf', pdf));
    for (const cell of ['Item', 'Qty', 'Price', 'Widget', 'Gadget', '19.99']) {
      assert.ok(text.includes(cell), `cell ${cell} not found in content stream`);
    }
  });

  it('wraps long cell text inside its column', () => {
    const pdf = render((d) => {
      const t = new PdfTable([60, 60], 10).setHeader('A', 'B').addRow('a very long cell that has to wrap across several lines', 'x');
      d.addTable(t);
      t.dispose();
    });
    checked('table_wrap.pdf', pdf);
  });

  it('splits a long table across pages and repeats the header', () => {
    const pdf = render((d) => {
      const t = new PdfTable([100, 100], 10).setHeader('HEADERCELL', 'Value');
      for (let i = 0; i < 150; i++) t.addRow(`row ${i}`, String(i));
      d.addTable(t);
      t.dispose();
    });
    const path = checked('table_pages.pdf', pdf);
    const pages = pageCount(path);
    assert.ok(pages >= 2, `expected a table spanning pages, got ${pages}`);
    assert.equal(qdf(path).split('HEADERCELL').length - 1, pages, 'header repeats once per page');
  });

  it('copies the table into the document, so disposing it afterwards is safe', () => {
    const t = sample();
    const doc = new PdfDocument();
    try {
      doc.addTable(t);
      t.dispose();
      t.dispose();
      checked('table_disposed.pdf', doc.render());
    } finally {
      doc.dispose();
    }
  });

  it('rejects strings with an embedded NUL instead of truncating them', () => {
    const doc = new PdfDocument();
    const t = new PdfTable([50, 50]);
    try {
      assert.throws(() => doc.addText('AAA\0BBB'), (e) => e instanceof TypeError && /text/.test(e.message) && /NUL/.test(e.message));
      assert.throws(() => t.setHeader('ok', 'A\0B'), (e) => e instanceof TypeError && /cells\[1\]/.test(e.message));
      assert.throws(() => t.addRow('A\0B'), (e) => e instanceof TypeError && /cells\[0\]/.test(e.message));
    } finally {
      t.dispose();
      doc.dispose();
    }
  });

  it('sizes the table from the widths it copied, not from a lying array', () => {
    class Liar extends Array {
      *[Symbol.iterator]() {
        yield 40; // one width, although length says four
      }
    }
    const t = new PdfTable(Liar.of(10, 20, 30, 40));
    const doc = new PdfDocument();
    try {
      t.addRow('a', 'b'); // two cells: only a one-column table rejects this at layout
      doc.addTable(t);
      assert.throws(() => doc.render(), (e) => e instanceof PdfError && e.code === -4 && /more cells than columns/.test(e.message));
    } finally {
      t.dispose();
      doc.dispose();
    }
  });

  it('rejects bad table arguments', () => {
    assert.throws(() => new PdfTable([]), (e) => e instanceof PdfError && e.code === -3);
    assert.throws(() => new PdfTable([0], 10), PdfError);
    assert.throws(() => new PdfTable([100], -1), PdfError);
    const t = new PdfTable([50, 50]);
    try {
      assert.throws(() => t.addRow('a', 1), TypeError);
      t.addRow('short row is fine');
      // A row with more cells than columns is only detected when the document is laid out.
      t.addRow('a', 'b', 'c');
      const doc = new PdfDocument();
      try {
        doc.addTable(t);
        assert.throws(() => doc.render(), (e) => e instanceof PdfError && e.code === -4 && /more cells than columns/.test(e.message));
      } finally {
        doc.dispose();
      }
    } finally {
      t.dispose();
    }
  });
});

describe('images', () => {
  it('embeds RGB and gray raw pixels as Flate image XObjects', () => {
    const pdf = render((d) => {
      d.addRawImage(16, 16, 3, gradient(16, 16, 3), 100).addRawImage(8, 8, 1, gradient(8, 8, 1), 50);
    });
    const text = qdf(checked('images.pdf', pdf));
    assert.match(text, /\/Subtype \/Image/);
    assert.match(text, /\/ColorSpace \/DeviceRGB/);
    assert.match(text, /\/ColorSpace \/DeviceGray/);
    assert.match(text, /\/Width 16/);
  });

  it('embeds a JPEG unchanged, and qpdf can decode it', () => {
    const jpeg = grayJpeg(64, 32);
    const path = checked('jpeg.pdf', render((d) => d.addJpeg(jpeg, 128)));
    const raw = readFileSync(path);
    assert.ok(raw.includes(jpeg), 'jpeg bytes embedded verbatim');
    const text = raw.toString('latin1');
    assert.match(text, /\/Filter \/DCTDecode/);
    assert.match(text, /\/ColorSpace \/DeviceGray/);
    assert.match(text, /\/Width 64 \/Height 32/);
  });

  it('accepts a Uint8Array as well as a Buffer', () => {
    checked('image_u8.pdf', render((d) => d.addRawImage(4, 4, 1, new Uint8Array(16), 40)));
  });

  it('sends the real size of a Uint8Array subclass that lies about its length', () => {
    class Liar extends Uint8Array {
      get length() {
        return 100000; // far past the 12 real bytes
      }
    }
    checked('image_liar.pdf', render((d) => d.addRawImage(2, 2, 3, new Liar(12), 40)));
  });

  it('rejects invalid images with the native message', () => {
    const doc = new PdfDocument();
    try {
      assert.throws(() => doc.addRawImage(4, 4, 2, new Uint8Array(32), 40), (e) => e instanceof PdfError && /channels/.test(e.message));
      assert.throws(() => doc.addRawImage(4, 4, 3, new Uint8Array(5), 40), (e) => e instanceof PdfError && e.code === -4);
      assert.throws(() => doc.addRawImage(4, 4, 1, new Uint8Array(16), 0), PdfError);
      assert.throws(() => doc.addJpeg(Buffer.from('not a jpeg'), 40), PdfError);
      assert.throws(() => doc.addRawImage(-1, 4, 1, new Uint8Array(16), 40), RangeError);
      assert.throws(() => doc.addRawImage(4, 4, 1, 'pixels', 40), TypeError);
    } finally {
      doc.dispose();
    }
  });
});

describe('page breaks and multi-page output', () => {
  it('starts a new page at each explicit break', () => {
    const pdf = render((d) => d.addText('page one').addPageBreak().addText('page two').addPageBreak().addText('page three'));
    assert.equal(pageCount(checked('breaks.pdf', pdf)), 3);
  });

  it('builds a mixed report over several pages', () => {
    const jpeg = grayJpeg(32, 32);
    const pdf = render((d) => {
      d.addText('Quarterly report', Font.HelveticaBold, 20).addText('Summary text.', Font.Helvetica, 11);
      const t = new PdfTable([140, 90, 90], 10).setHeader('Region', 'Units', 'Revenue');
      for (let i = 0; i < 60; i++) t.addRow(`Region ${i}`, String(i * 3), (i * 12.5).toFixed(2));
      d.addTable(t);
      t.dispose();
      d.addPageBreak()
        .addRawImage(16, 16, 3, gradient(16, 16, 3), 120)
        .addJpeg(jpeg, 80)
        .addText('const sample = 1;', Font.Courier, 9);
    });
    const path = checked('report.pdf', pdf);
    assert.ok(pageCount(path) >= 3);
    const text = qdf(path);
    assert.match(text, /\/BaseFont \/Helvetica-Bold/);
    assert.match(text, /\/Subtype \/Image/);
  });

  it('can render the same document twice with identical bytes', () => {
    const doc = new PdfDocument();
    try {
      doc.addText('twice');
      assert.deepEqual(doc.render(), doc.render());
    } finally {
      doc.dispose();
    }
  });
});

describe('lifecycle', () => {
  it('throws on a disposed object and tolerates double dispose', () => {
    const doc = new PdfDocument();
    doc.dispose();
    doc.dispose();
    assert.throws(() => doc.addText('x'), /disposed/);
    assert.throws(() => doc.render(), /disposed/);
    const t = new PdfTable([10]);
    t.dispose();
    assert.throws(() => t.addRow('x'), /disposed/);
  });

  it('disposes through `using`', () => {
    let kept;
    {
      using doc = new PdfDocument();
      kept = doc;
      doc.addText('scoped');
    }
    assert.throws(() => kept.render(), /disposed/);
  });

  it('rejects a non-table in addTable and a non-string text', () => {
    const doc = new PdfDocument();
    try {
      assert.throws(() => doc.addTable({}), TypeError);
      assert.throws(() => doc.addText(null), TypeError);
    } finally {
      doc.dispose();
    }
  });

  it('builds many independent documents', () => {
    for (let i = 0; i < 200; i++) {
      const pdf = render((d) => d.addText(`doc ${i}`, Font.Courier, 10));
      assert.equal(pdf.subarray(0, 5).toString('latin1'), '%PDF-');
    }
  });
});

describe('native library lookup', () => {
  const nodeDir = join(dirname(fileURLToPath(import.meta.url)), '..');
  const libName = `${process.platform === 'win32' ? '' : 'lib'}pdfgen_ffi${{ win32: '.dll', darwin: '.dylib' }[process.platform] ?? '.so'}`;

  /**
   * Imports a copy of native.js placed at `<scratch>/<dir>/native.js`, in a scratch repo that has an ffi/Cargo.toml
   * and a file where the checkout layout would look for the library, with PDFGEN_LIB unset.
   */
  function importCopy(dir) {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'pdfgen-lookup-')));
    try {
      mkdirSync(join(root, dir), { recursive: true });
      cpSync(join(nodeDir, 'src', 'native.js'), join(root, dir, 'native.js'));
      symlinkSync(join(nodeDir, 'node_modules'), join(root, 'node_modules'), 'dir'); // so `koffi` resolves
      mkdirSync(join(root, 'ffi'));
      writeFileSync(join(root, 'ffi', 'Cargo.toml'), '[package]\nname = "unrelated"\n');
      mkdirSync(join(root, 'target', 'release'), { recursive: true });
      writeFileSync(join(root, 'target', 'release', libName), 'not a library');
      const env = { ...process.env };
      delete env.PDFGEN_LIB;
      const url = pathToFileURL(join(root, dir, 'native.js')).href;
      return spawnSync(process.execPath, ['--input-type=module', '-e', `await import(${JSON.stringify(url)})`], {
        encoding: 'utf8',
        env,
        cwd: root,
      });
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }

  it('does not load a library next to an unrelated workspace for a copy two levels below it', () => {
    const r = importCopy('vendor/src');
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /not in a source checkout/);
    assert.doesNotMatch(r.stderr, /Failed to load shared library/); // the planted file never reached dlopen
  });

  it('still selects the default path for the real <repo>/node/src layout', () => {
    const r = importCopy('node/src');
    assert.notEqual(r.status, 0);
    // The layout matched, so koffi was handed the planted file and rejected it.
    assert.match(r.stderr, /Failed to load shared library/);
    assert.doesNotMatch(r.stderr, /not in a source checkout/);
  });
});
