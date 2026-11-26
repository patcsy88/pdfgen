package io.github.patcsy88.pdfgen;

import static io.github.patcsy88.pdfgen.Support.checked;
import static io.github.patcsy88.pdfgen.Support.contains;
import static io.github.patcsy88.pdfgen.Support.grayJpeg;
import static io.github.patcsy88.pdfgen.Support.gradient;
import static io.github.patcsy88.pdfgen.Support.pageCount;
import static io.github.patcsy88.pdfgen.Support.qdf;
import static io.github.patcsy88.pdfgen.Support.render;
import static org.testng.Assert.assertEquals;
import static org.testng.Assert.assertTrue;
import static org.testng.Assert.expectThrows;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.testng.annotations.Test;

public class TableImageTest {
    private static PdfTable sample() {
        return new PdfTable(new float[] {120, 80, 80}, 10).setHeader("Item", "Qty", "Price")
                .addRow("Widget", "4", "3.50").addRow("Gadget", "12", "19.99");
    }

    @Test
    public void rendersHeaderCellsAndBorders() {
        byte[] pdf = render(d -> {
            try (PdfTable t = sample()) {
                d.addTable(t);
            }
        });
        String text = qdf(checked("table.pdf", pdf));
        for (String cell : new String[] {"Item", "Qty", "Price", "Widget", "Gadget", "19.99"}) {
            assertTrue(text.contains(cell), "missing " + cell);
        }
    }

    @Test
    public void wrapsLongCellTextInsideItsColumn() {
        byte[] pdf = render(d -> {
            try (PdfTable t = new PdfTable(new float[] {60, 60}, 10)) {
                t.setHeader("A", "B").addRow("a very long cell that has to wrap across several lines", "x");
                d.addTable(t);
            }
        });
        checked("table_wrap.pdf", pdf);
    }

    @Test
    public void splitsALongTableAcrossPagesAndRepeatsTheHeader() {
        byte[] pdf = render(d -> {
            try (PdfTable t = new PdfTable(new float[] {100, 100}, 10)) {
                t.setHeader("HEADERCELL", "Value");
                for (int i = 0; i < 150; i++) {
                    t.addRow("row " + i, String.valueOf(i));
                }
                d.addTable(t);
            }
        });
        Path path = checked("table_pages.pdf", pdf);
        int pages = pageCount(path);
        assertTrue(pages >= 2);
        String text = qdf(path);
        assertEquals(text.split("HEADERCELL", -1).length - 1, pages, "header repeats once per page");
    }

    @Test
    public void copiesTheTableSoClosingItAfterwardsIsSafe() {
        PdfTable t = sample();
        try (PdfDocument doc = new PdfDocument()) {
            doc.addTable(t);
            t.close();
            t.close();
            checked("table_closed.pdf", doc.render());
        }
    }

    @Test
    public void rejectsBadTableArguments() {
        PdfException e = expectThrows(PdfException.class, () -> new PdfTable(new float[0]));
        assertEquals(e.code(), -3);
        expectThrows(PdfException.class, () -> new PdfTable(new float[] {0f}, 10));
        expectThrows(PdfException.class, () -> new PdfTable(new float[] {100f}, -1));
        try (PdfTable t = new PdfTable(new float[] {50, 50}, 10)) {
            expectThrows(NullPointerException.class, () -> t.addRow("a", null));
            expectThrows(IllegalArgumentException.class, () -> t.addRow("a\0b"));
            t.addRow("a short row is fine");
            // A row with more cells than columns is only detected when the document is laid out.
            t.addRow("a", "b", "c");
            try (PdfDocument doc = new PdfDocument()) {
                doc.addTable(t);
                PdfException laid = expectThrows(PdfException.class, doc::render);
                assertEquals(laid.code(), -4);
                assertTrue(laid.getMessage().contains("more cells than columns"), laid.getMessage());
            }
        }
    }

    @Test
    public void embedsRgbAndGrayRawPixelsAsFlateImageXObjects() {
        byte[] pdf = render(d -> d.addRawImage(16, 16, 3, gradient(16, 16, 3), 100)
                .addRawImage(8, 8, 1, gradient(8, 8, 1), 50));
        String text = qdf(checked("images.pdf", pdf));
        assertTrue(text.contains("/Subtype /Image"));
        assertTrue(text.contains("/ColorSpace /DeviceRGB"));
        assertTrue(text.contains("/ColorSpace /DeviceGray"));
        assertTrue(text.contains("/Width 16"));
    }

    @Test
    public void embedsAJpegUnchangedAndQpdfCanDecodeIt() throws Exception {
        byte[] jpeg = grayJpeg(64, 32);
        Path path = checked("jpeg.pdf", render(d -> d.addJpeg(jpeg, 128)));
        byte[] raw = Files.readAllBytes(path);
        assertTrue(contains(raw, jpeg), "jpeg bytes embedded verbatim");
        String text = new String(raw, StandardCharsets.ISO_8859_1);
        assertTrue(text.contains("/Filter /DCTDecode"));
        assertTrue(text.contains("/ColorSpace /DeviceGray"));
        assertTrue(text.contains("/Width 64 /Height 32"));
    }

    @Test
    public void rejectsInvalidImagesWithTheNativeMessage() {
        try (PdfDocument doc = new PdfDocument()) {
            PdfException bad = expectThrows(PdfException.class, () -> doc.addRawImage(4, 4, 2, new byte[32], 40));
            assertTrue(bad.getMessage().contains("channels"), bad.getMessage());
            PdfException size = expectThrows(PdfException.class, () -> doc.addRawImage(4, 4, 3, new byte[5], 40));
            assertEquals(size.code(), -4);
            expectThrows(PdfException.class, () -> doc.addRawImage(4, 4, 1, new byte[16], 0));
            expectThrows(PdfException.class, () -> doc.addJpeg("not a jpeg".getBytes(StandardCharsets.UTF_8), 40));
            expectThrows(IllegalArgumentException.class, () -> doc.addRawImage(-1, 4, 1, new byte[16], 40));
            expectThrows(NullPointerException.class, () -> doc.addRawImage(4, 4, 1, null, 40));
        }
    }
}
