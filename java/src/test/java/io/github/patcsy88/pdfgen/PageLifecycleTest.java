package io.github.patcsy88.pdfgen;

import static io.github.patcsy88.pdfgen.Support.OUT;
import static io.github.patcsy88.pdfgen.Support.checked;
import static io.github.patcsy88.pdfgen.Support.grayJpeg;
import static io.github.patcsy88.pdfgen.Support.gradient;
import static io.github.patcsy88.pdfgen.Support.pageCount;
import static io.github.patcsy88.pdfgen.Support.qdf;
import static io.github.patcsy88.pdfgen.Support.render;
import static org.testng.Assert.assertEquals;
import static org.testng.Assert.assertTrue;
import static org.testng.Assert.expectThrows;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import org.testng.annotations.Test;

public class PageLifecycleTest {
    @Test
    public void startsANewPageAtEachExplicitBreak() {
        byte[] pdf = render(d -> d.addText("page one").addPageBreak().addText("page two").addPageBreak()
                .addText("page three"));
        assertEquals(pageCount(checked("breaks.pdf", pdf)), 3);
    }

    @Test
    public void buildsAMixedReportOverSeveralPages() {
        byte[] jpeg = grayJpeg(32, 32);
        byte[] pdf = render(d -> {
            d.addText("Quarterly report", Font.HELVETICA_BOLD, 20).addText("Summary text.", Font.HELVETICA, 11);
            try (PdfTable t = new PdfTable(new float[] {140, 90, 90}, 10)) {
                t.setHeader("Region", "Units", "Revenue");
                for (int i = 0; i < 60; i++) {
                    t.addRow("Region " + i, String.valueOf(i * 3), String.format("%.2f", i * 12.5));
                }
                d.addTable(t);
            }
            d.addPageBreak().addRawImage(16, 16, 3, gradient(16, 16, 3), 120).addJpeg(jpeg, 80);
            d.addText("x = 1", Font.COURIER, 9);
        });
        Path path = checked("report.pdf", pdf);
        assertTrue(pageCount(path) >= 3);
        String text = qdf(path);
        assertTrue(text.contains("/BaseFont /Helvetica-Bold"));
        assertTrue(text.contains("/Subtype /Image"));
    }

    @Test
    public void rendersTheSameDocumentTwiceWithIdenticalBytes() {
        try (PdfDocument doc = new PdfDocument()) {
            doc.addText("twice");
            assertEquals(doc.render(), doc.render());
        }
    }

    @Test
    public void saveWritesTheSameBytesAsRender() throws Exception {
        try (PdfDocument doc = new PdfDocument()) {
            doc.addText("saved");
            doc.save(OUT.resolve("saved.pdf"));
            byte[] written = Files.readAllBytes(OUT.resolve("saved.pdf"));
            assertEquals(written, doc.render());
            checked("saved_check.pdf", written);
        }
    }

    @Test
    public void closedObjectsThrowAndDoubleCloseIsSafe() {
        PdfDocument doc = new PdfDocument();
        doc.close();
        doc.close();
        IllegalStateException e = expectThrows(IllegalStateException.class, () -> doc.addText("x"));
        assertTrue(e.getMessage().contains("closed"));
        expectThrows(IllegalStateException.class, doc::render);
        PdfTable t = new PdfTable(new float[] {10});
        t.close();
        expectThrows(IllegalStateException.class, () -> t.addRow("x"));
        try (PdfDocument other = new PdfDocument()) {
            expectThrows(IllegalStateException.class, () -> other.addTable(t));
            expectThrows(NullPointerException.class, () -> other.addTable(null));
        }
    }

    @Test
    public void buildsManyIndependentDocuments() {
        for (int i = 0; i < 200; i++) {
            final int n = i;
            byte[] pdf = render(d -> d.addText("doc " + n, Font.COURIER, 10));
            assertEquals(pdf[0], (byte) '%');
            assertEquals(pdf[4], (byte) '-');
        }
    }

    @Test
    public void errorMessagesArePerThread() throws Exception {
        List<String> messages = Collections.synchronizedList(new ArrayList<>());
        List<Thread> threads = new ArrayList<>();
        for (int i = 0; i < 8; i++) {
            Thread th = new Thread(() -> {
                try (PdfDocument doc = new PdfDocument()) {
                    doc.addText("x", Font.HELVETICA, -1f);
                } catch (PdfException e) {
                    messages.add(e.getMessage());
                }
            });
            threads.add(th);
            th.start();
        }
        for (Thread th : threads) {
            th.join();
        }
        assertEquals(messages.size(), 8);
        assertEquals(messages.stream().distinct().count(), 1L, "every thread reads its own message: " + messages);
    }

    @Test
    public void manyDocumentsOnManyThreadsRenderValidPdfs() throws Exception {
        List<byte[]> pdfs = Collections.synchronizedList(new ArrayList<>());
        List<Thread> threads = new ArrayList<>();
        for (int i = 0; i < 8; i++) {
            final int n = i;
            Thread th = new Thread(() -> {
                for (int j = 0; j < 25; j++) {
                    final int m = j;
                    pdfs.add(render(d -> d.addText("thread " + n + " doc " + m)));
                }
            });
            threads.add(th);
            th.start();
        }
        for (Thread th : threads) {
            th.join();
        }
        assertEquals(pdfs.size(), 200);
        for (byte[] p : pdfs) {
            assertEquals(new String(p, 0, 8, java.nio.charset.StandardCharsets.ISO_8859_1), "%PDF-2.0");
        }
    }
}
