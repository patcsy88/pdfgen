package io.github.patcsy88.pdfgen;

import static io.github.patcsy88.pdfgen.Support.checked;
import static io.github.patcsy88.pdfgen.Support.pageCount;
import static io.github.patcsy88.pdfgen.Support.qdf;
import static io.github.patcsy88.pdfgen.Support.render;
import static org.testng.Assert.assertEquals;
import static org.testng.Assert.assertTrue;
import static org.testng.Assert.expectThrows;

import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import org.testng.annotations.Test;

public class TextFontTest {
    @Test
    public void reportsNativeVersion() {
        assertTrue(PdfDocument.nativeVersion().matches("^\\d+\\.\\d+\\.\\d+.*"));
    }

    @Test
    public void drawsLatin1TextAsPdf20() {
        byte[] pdf = render(d -> d.addText("Hello from Java, caf\u00e9", Font.HELVETICA, 14));
        assertEquals(new String(pdf, 0, 8, StandardCharsets.ISO_8859_1), "%PDF-2.0");
        Path path = checked("text.pdf", pdf);
        assertTrue(qdf(path).contains("/F1 14 Tf"));
        assertEquals(pageCount(path), 1);
    }

    @Test
    public void wrapsLongTextOntoFurtherPages() {
        byte[] pdf = render(d -> {
            for (int i = 0; i < 120; i++) {
                d.addText("Line " + i + ": the quick brown fox jumps over the lazy dog.");
            }
        });
        assertTrue(pageCount(checked("flow.pdf", pdf)) >= 2);
    }

    @Test
    public void isByteIdenticalForIdenticalInput() {
        assertEquals(render(d -> d.addText("same", Font.COURIER, 10)),
                render(d -> d.addText("same", Font.COURIER, 10)));
    }

    @Test
    public void rendersAnEmptyDocumentAsValidPdf() {
        assertEquals(pageCount(checked("empty.pdf", render(d -> { }))), 1);
    }

    @Test
    public void textOutsideLatin1StillRenders() {
        checked("text_unicode.pdf", render(d -> d.addText("snowman \u2603 and kanji \u6f22")));
    }

    @Test
    public void declaresAndUsesAllFourBuiltinFaces() {
        byte[] pdf = render(d -> d.addText("sans", Font.HELVETICA, 12).addText("sans bold", Font.HELVETICA_BOLD, 12)
                .addText("mono", Font.COURIER, 12).addText("mono bold", Font.COURIER_BOLD, 12));
        String text = qdf(checked("fonts.pdf", pdf));
        for (String base : new String[] {"Helvetica", "Helvetica-Bold", "Courier", "Courier-Bold"}) {
            assertTrue(text.contains("/BaseFont /" + base + "\n") || text.contains("/BaseFont /" + base + " "),
                    "missing " + base);
        }
        for (String f : new String[] {"F1", "F2", "F3", "F4"}) {
            assertTrue(text.contains("/" + f + " 12 Tf"), "missing " + f);
        }
    }

    @Test
    public void rejectsBadSizesWithTheNativeMessage() {
        try (PdfDocument doc = new PdfDocument()) {
            for (float size : new float[] {0f, -1f, Float.NaN, Float.POSITIVE_INFINITY}) {
                PdfException e = expectThrows(PdfException.class, () -> doc.addText("x", Font.HELVETICA, size));
                assertEquals(e.code(), -3, "size " + size);
                assertTrue(!e.getMessage().isEmpty());
            }
        }
    }

    @Test
    public void rejectsTextThatTheCLayerWouldTruncate() {
        try (PdfDocument doc = new PdfDocument()) {
            expectThrows(IllegalArgumentException.class, () -> doc.addText("before\0after"));
            expectThrows(NullPointerException.class, () -> doc.addText(null));
            expectThrows(NullPointerException.class, () -> doc.addText("x", null, 12f));
        }
    }
}
