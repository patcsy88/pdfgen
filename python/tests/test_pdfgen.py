"""Tests for the ctypes wrapper. Every PDF is written to python/test-output/ and must pass
`qpdf --check`; the Arlington and content-stream checks then run over the whole directory via
`PDFGEN_CHECK_DIR=python/test-output cargo test -p pdfgen-core --test external_pdfs`.
qpdf is not a PDF 2.0 conformance validator; see README.md for what the checks cover.
"""

import copy
import re
import shutil
import struct
import subprocess
import threading
import unittest
from pathlib import Path

from pdfgen import Font, PdfDocument, PdfError, PdfTable, native_version

OUT = Path(__file__).resolve().parents[1] / "test-output"


def setUpModule() -> None:
    if shutil.which("qpdf") is None:
        raise RuntimeError("qpdf must be installed to run these tests (apt install qpdf)")
    shutil.rmtree(OUT, ignore_errors=True)
    OUT.mkdir(parents=True)


def qpdf(*args: str) -> tuple[int, str]:
    r = subprocess.run(["qpdf", *args], capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


def checked(name: str, pdf: bytes) -> Path:
    """Writes the PDF to test-output/, requires `qpdf --check` to exit 0, returns the path."""
    path = OUT / name
    path.write_bytes(pdf)
    status, output = qpdf("--check", str(path))
    assert status == 0, f"qpdf --check exited {status} for {name}:\n{output}"
    return path


def page_count(path: Path) -> int:
    status, output = qpdf("--show-npages", str(path))
    assert status == 0, output
    return int(output.strip())


def qdf(path: Path) -> str:
    """The PDF with all streams and object streams decompressed, as text."""
    out = path.with_suffix(".qdf")
    status, output = qpdf("--qdf", "--object-streams=disable", str(path), str(out))
    assert status in (0, 3), output
    text = out.read_text("latin-1")
    out.unlink()
    return text


def gradient(w: int, h: int, channels: int) -> bytes:
    return bytes((i * 7) & 0xFF for i in range(w * h * channels))


def gray_jpeg(w: int, h: int) -> bytes:
    """Minimal valid baseline grayscale JPEG (flat mid-gray); w and h multiples of 8.

    One 8x8 block is a DC code "0" plus an EOB code "0", each a 1-bit Huffman code.
    Port of `gray_jpeg` in core/tests/pdf.rs, so qpdf can decode the DCT stream.
    """
    assert w % 8 == 0 and h % 8 == 0
    bits = (w // 8) * (h // 8) * 2
    data = bytearray((bits + 7) // 8)
    pad = len(data) * 8 - bits
    if pad:
        data[-1] |= (1 << pad) - 1
    out = bytearray(b"\xff\xd8\xff\xdb\x00\x43\x00" + b"\x01" * 64)
    out += b"\xff\xc0\x00\x0b\x08" + struct.pack(">HH", h, w) + b"\x01\x01\x11\x00"
    for class_id in (0x00, 0x10):
        out += bytes([0xFF, 0xC4, 0x00, 0x14, class_id, 0x01]) + bytes(15) + b"\x00"
    out += b"\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00" + bytes(data) + b"\xff\xd9"
    return bytes(out)


def render(build) -> bytes:
    with PdfDocument() as doc:
        build(doc)
        return doc.render()


class TextTests(unittest.TestCase):
    def test_reports_native_version(self):
        self.assertRegex(native_version(), r"^\d+\.\d+\.\d+")

    def test_draws_text_including_non_ascii_latin1_as_pdf_2_0(self):
        pdf = render(lambda d: d.add_text("Hello from Python, café", Font.HELVETICA, 14))
        self.assertEqual(pdf[:8], b"%PDF-2.0")
        path = checked("text.pdf", pdf)
        self.assertIn("/F1 14 Tf", qdf(path))
        self.assertEqual(page_count(path), 1)

    def test_wraps_long_text_onto_further_pages(self):
        def build(d):
            for i in range(120):
                d.add_text(f"Line {i}: the quick brown fox jumps over the lazy dog.")

        self.assertGreaterEqual(page_count(checked("flow.pdf", render(build))), 2)

    def test_is_byte_identical_for_identical_input(self):
        make = lambda: render(lambda d: d.add_text("same", Font.COURIER, 10))
        self.assertEqual(make(), make())

    def test_renders_an_empty_document_as_valid_pdf(self):
        self.assertEqual(page_count(checked("empty.pdf", render(lambda d: None))), 1)

    def test_text_outside_the_latin1_range_still_renders(self):
        checked("text_unicode.pdf", render(lambda d: d.add_text("snowman \u2603 and kanji \u6f22")))


class FontTests(unittest.TestCase):
    def test_declares_and_uses_all_four_builtin_faces(self):
        def build(d):
            d.add_text("sans", Font.HELVETICA).add_text("sans bold", Font.HELVETICA_BOLD)
            d.add_text("mono", Font.COURIER).add_text("mono bold", Font.COURIER_BOLD)

        text = qdf(checked("fonts.pdf", render(build)))
        for base in ("Helvetica", "Helvetica-Bold", "Courier", "Courier-Bold"):
            self.assertIn(f"/BaseFont /{base}", text)
        for f in ("F1", "F2", "F3", "F4"):
            self.assertIn(f"/{f} 12 Tf", text)

    def test_plain_ints_work_as_font_codes(self):
        checked("font_int.pdf", render(lambda d: d.add_text("x", 2, 10)))

    def test_rejects_unknown_font_code_and_bad_sizes_with_the_native_message(self):
        with PdfDocument() as doc:
            with self.assertRaisesRegex(PdfError, "font") as cm:
                doc.add_text("x", 9)
            self.assertEqual(cm.exception.code, -3)
            for size in (0, -1, float("nan"), float("inf")):
                with self.subTest(size=size), self.assertRaises(PdfError) as cm:
                    doc.add_text("x", Font.HELVETICA, size)
                self.assertEqual(cm.exception.code, -3)

    def test_rejects_text_that_ctypes_would_truncate(self):
        with PdfDocument() as doc:
            with self.assertRaises(ValueError):
                doc.add_text("before\0after")
            with self.assertRaises(TypeError):
                doc.add_text(b"bytes")


class TableTests(unittest.TestCase):
    @staticmethod
    def sample() -> PdfTable:
        return (
            PdfTable([120, 80, 80], 10)
            .set_header("Item", "Qty", "Price")
            .add_row("Widget", "4", "3.50")
            .add_row("Gadget", "12", "19.99")
        )

    def test_renders_header_cells_and_borders(self):
        def build(d):
            with self.sample() as t:
                d.add_table(t)

        text = qdf(checked("table.pdf", render(build)))
        for cell in ("Item", "Qty", "Price", "Widget", "Gadget", "19.99"):
            self.assertIn(cell, text)

    def test_wraps_long_cell_text_inside_its_column(self):
        def build(d):
            with PdfTable([60, 60], 10) as t:
                t.set_header("A", "B").add_row("a very long cell that has to wrap across several lines", "x")
                d.add_table(t)

        checked("table_wrap.pdf", render(build))

    def test_splits_a_long_table_across_pages_and_repeats_the_header(self):
        def build(d):
            with PdfTable([100, 100], 10) as t:
                t.set_header("HEADERCELL", "Value")
                for i in range(150):
                    t.add_row(f"row {i}", str(i))
                d.add_table(t)

        path = checked("table_pages.pdf", render(build))
        pages = page_count(path)
        self.assertGreaterEqual(pages, 2)
        self.assertEqual(qdf(path).count("HEADERCELL"), pages, "header repeats once per page")

    def test_copies_the_table_so_closing_it_afterwards_is_safe(self):
        t = self.sample()
        with PdfDocument() as doc:
            doc.add_table(t)
            t.close()
            t.close()
            checked("table_closed.pdf", doc.render())

    def test_rejects_bad_table_arguments(self):
        with self.assertRaises(PdfError) as cm:
            PdfTable([])
        self.assertEqual(cm.exception.code, -3)
        with self.assertRaises(PdfError):
            PdfTable([0], 10)
        with self.assertRaises(PdfError):
            PdfTable([100], -1)
        with PdfTable([50, 50]) as t:
            with self.assertRaises(TypeError):
                t.add_row("a", 1)
            t.add_row("a short row is fine")
            # A row with more cells than columns is only detected when the document is laid out.
            t.add_row("a", "b", "c")
            with PdfDocument() as doc:
                doc.add_table(t)
                with self.assertRaisesRegex(PdfError, "more cells than columns") as cm:
                    doc.render()
                self.assertEqual(cm.exception.code, -4)


class ImageTests(unittest.TestCase):
    def test_embeds_rgb_and_gray_raw_pixels_as_flate_image_xobjects(self):
        def build(d):
            d.add_raw_image(16, 16, 3, gradient(16, 16, 3), 100)
            d.add_raw_image(8, 8, 1, gradient(8, 8, 1), 50)

        text = qdf(checked("images.pdf", render(build)))
        self.assertIn("/Subtype /Image", text)
        self.assertIn("/ColorSpace /DeviceRGB", text)
        self.assertIn("/ColorSpace /DeviceGray", text)
        self.assertIn("/Width 16", text)

    def test_embeds_a_jpeg_unchanged_and_qpdf_can_decode_it(self):
        jpeg = gray_jpeg(64, 32)
        path = checked("jpeg.pdf", render(lambda d: d.add_jpeg(jpeg, 128)))
        raw = path.read_bytes()
        self.assertIn(jpeg, raw, "jpeg bytes embedded verbatim")
        text = raw.decode("latin-1")
        self.assertIn("/Filter /DCTDecode", text)
        self.assertIn("/ColorSpace /DeviceGray", text)
        self.assertIn("/Width 64 /Height 32", text)

    def test_accepts_bytearray_and_memoryview_as_well_as_bytes(self):
        checked("image_bytearray.pdf", render(lambda d: d.add_raw_image(4, 4, 1, bytearray(16), 40)))
        checked("image_memoryview.pdf", render(lambda d: d.add_raw_image(4, 4, 1, memoryview(bytes(16)), 40)))

    def test_rejects_invalid_images_with_the_native_message(self):
        with PdfDocument() as doc:
            with self.assertRaisesRegex(PdfError, "channels"):
                doc.add_raw_image(4, 4, 2, bytes(32), 40)
            with self.assertRaises(PdfError) as cm:
                doc.add_raw_image(4, 4, 3, bytes(5), 40)
            self.assertEqual(cm.exception.code, -4)
            with self.assertRaises(PdfError):
                doc.add_raw_image(4, 4, 1, bytes(16), 0)
            with self.assertRaises(PdfError):
                doc.add_jpeg(b"not a jpeg", 40)
            with self.assertRaises(ValueError):
                doc.add_raw_image(-1, 4, 1, bytes(16), 40)
            with self.assertRaises(ValueError):
                doc.add_raw_image(2**32, 4, 1, bytes(16), 40)
            with self.assertRaises(TypeError):
                doc.add_raw_image(4, 4, 1, "pixels", 40)


class PageTests(unittest.TestCase):
    def test_starts_a_new_page_at_each_explicit_break(self):
        pdf = render(lambda d: d.add_text("page one").add_page_break().add_text("page two").add_page_break().add_text("page three"))
        self.assertEqual(page_count(checked("breaks.pdf", pdf)), 3)

    def test_builds_a_mixed_report_over_several_pages(self):
        jpeg = gray_jpeg(32, 32)

        def build(d):
            d.add_text("Quarterly report", Font.HELVETICA_BOLD, 20).add_text("Summary text.", Font.HELVETICA, 11)
            with PdfTable([140, 90, 90], 10) as t:
                t.set_header("Region", "Units", "Revenue")
                for i in range(60):
                    t.add_row(f"Region {i}", str(i * 3), f"{i * 12.5:.2f}")
                d.add_table(t)
            d.add_page_break().add_raw_image(16, 16, 3, gradient(16, 16, 3), 120).add_jpeg(jpeg, 80)
            d.add_text("x = 1", Font.COURIER, 9)

        path = checked("report.pdf", render(build))
        self.assertGreaterEqual(page_count(path), 3)
        text = qdf(path)
        self.assertIn("/BaseFont /Helvetica-Bold", text)
        self.assertIn("/Subtype /Image", text)

    def test_can_render_the_same_document_twice_with_identical_bytes(self):
        with PdfDocument() as doc:
            doc.add_text("twice")
            self.assertEqual(doc.render(), doc.render())

    def test_save_writes_the_same_bytes_as_render(self):
        with PdfDocument() as doc:
            doc.add_text("saved")
            doc.save(OUT / "saved.pdf")
            self.assertEqual((OUT / "saved.pdf").read_bytes(), doc.render())
        checked("saved_check.pdf", (OUT / "saved.pdf").read_bytes())


class LifecycleTests(unittest.TestCase):
    def test_closed_objects_raise_and_double_close_is_safe(self):
        doc = PdfDocument()
        doc.close()
        doc.close()
        with self.assertRaisesRegex(ValueError, "closed"):
            doc.add_text("x")
        with self.assertRaisesRegex(ValueError, "closed"):
            doc.render()
        t = PdfTable([10])
        t.close()
        with self.assertRaisesRegex(ValueError, "closed"):
            t.add_row("x")

    def test_with_block_closes_the_document(self):
        with PdfDocument() as doc:
            doc.add_text("scoped")
        with self.assertRaisesRegex(ValueError, "closed"):
            doc.render()

    def test_rejects_a_non_table_and_non_string_text(self):
        with PdfDocument() as doc:
            with self.assertRaises(TypeError):
                doc.add_table({})
            with self.assertRaises(TypeError):
                doc.add_text(None)

    def test_a_closed_table_cannot_be_added(self):
        t = PdfTable([10])
        t.close()
        with PdfDocument() as doc, self.assertRaisesRegex(ValueError, "closed"):
            doc.add_table(t)

    def test_builds_many_independent_documents(self):
        for i in range(200):
            self.assertEqual(render(lambda d: d.add_text(f"doc {i}", Font.COURIER, 10))[:5], b"%PDF-")

    def test_error_messages_are_per_thread(self):
        results: list[str] = []

        def worker(code: int) -> None:
            with PdfDocument() as doc:
                try:
                    doc.add_text("x", code)
                except PdfError as e:
                    results.append(str(e))

        threads = [threading.Thread(target=worker, args=(100 + i,)) for i in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(results, ["font code out of range"] * 8)

    def test_many_documents_on_many_threads_render_valid_pdfs(self):
        pdfs: list[bytes] = []

        def worker(i: int) -> None:
            for j in range(25):
                pdfs.append(render(lambda d: d.add_text(f"thread {i} doc {j}")))

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(len(pdfs), 200)
        self.assertTrue(all(re.match(rb"%PDF-2\.0", p) for p in pdfs))

    def test_copying_or_pickling_a_handle_owner_is_refused(self):
        import pickle

        with PdfDocument() as doc, PdfTable([10]) as table:
            for obj in (doc, table):
                with self.assertRaises(TypeError):
                    copy.copy(obj)
                with self.assertRaises(TypeError):
                    copy.deepcopy(obj)
                with self.assertRaises(TypeError):
                    pickle.dumps(obj)
            checked("copy_refused.pdf", doc.add_text("still usable").render())

    def test_close_during_a_render_on_another_thread_waits_for_it(self):
        # The worker renders in a loop and close() starts only after its first render succeeded, so later
        # renders are very likely in flight when close() runs; each must return a valid PDF or raise closed.
        text = "lorem ipsum dolor sit amet " * 20000
        for _ in range(5):
            doc = PdfDocument()
            doc.add_text(text)
            started = threading.Event()
            problems: list[object] = []

            def render_loop() -> None:
                first = True
                while True:
                    try:
                        pdf = doc.render()
                    except ValueError:  # closed: the only acceptable way to stop
                        return
                    except BaseException as e:
                        problems.append(e)
                        started.set()
                        return
                    if not pdf.startswith(b"%PDF-2.0"):
                        problems.append(pdf[:8])
                        started.set()
                        return
                    if first:
                        first = False
                        started.set()

            t = threading.Thread(target=render_loop)
            t.start()
            self.assertTrue(started.wait(60), "the first render never finished")
            doc.close()
            t.join()
            self.assertEqual(problems, [])

    def test_concurrent_close_calls_free_the_handle_once(self):
        doc = PdfDocument()
        threads = [threading.Thread(target=doc.close) for _ in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        with self.assertRaisesRegex(ValueError, "closed"):
            doc.render()


if __name__ == "__main__":
    unittest.main()
