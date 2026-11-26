package io.github.patcsy88.pdfgen;

import static org.testng.Assert.assertEquals;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.function.Consumer;
import java.util.stream.Stream;

/**
 * Helpers for the tests. Every PDF goes to java/test-output/ and must pass {@code qpdf --check}; the Arlington and
 * content-stream checks then run over the directory via
 * {@code PDFGEN_CHECK_DIR=java/test-output cargo test -p pdfgen-core --test external_pdfs}.
 */
final class Support {
    static final Path OUT = Path.of("test-output").toAbsolutePath();

    static {
        try {
            if (Files.exists(OUT)) {
                try (Stream<Path> walk = Files.walk(OUT)) {
                    walk.sorted(Comparator.reverseOrder()).forEach(p -> p.toFile().delete());
                }
            }
            Files.createDirectories(OUT);
        } catch (IOException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    private Support() {
    }

    record Result(int status, String output) {
    }

    static Result qpdf(String... args) {
        String[] cmd = new String[args.length + 1];
        cmd[0] = "qpdf";
        System.arraycopy(args, 0, cmd, 1, args.length);
        try {
            Process p = new ProcessBuilder(cmd).redirectErrorStream(true).start();
            String out = new String(p.getInputStream().readAllBytes(), StandardCharsets.ISO_8859_1);
            return new Result(p.waitFor(), out);
        } catch (IOException e) {
            throw new IllegalStateException("qpdf must be installed to run these tests (apt install qpdf)", e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new IllegalStateException(e);
        }
    }

    /** Writes the PDF to test-output/, requires {@code qpdf --check} to exit 0, returns the path. */
    static Path checked(String name, byte[] pdf) {
        Path path = OUT.resolve(name);
        try {
            Files.write(path, pdf);
        } catch (IOException e) {
            throw new IllegalStateException(e);
        }
        Result r = qpdf("--check", path.toString());
        assertEquals(r.status(), 0, "qpdf --check exited " + r.status() + " for " + name + ":\n" + r.output());
        return path;
    }

    static int pageCount(Path path) {
        Result r = qpdf("--show-npages", path.toString());
        assertEquals(r.status(), 0, r.output());
        return Integer.parseInt(r.output().trim());
    }

    /** The PDF with all streams and object streams decompressed, as text. */
    static String qdf(Path path) {
        Path out = OUT.resolve(path.getFileName() + ".qdf.tmp");
        Result r = qpdf("--qdf", "--object-streams=disable", path.toString(), out.toString());
        if (r.status() != 0 && r.status() != 3) {
            throw new AssertionError(r.output());
        }
        try {
            String text = Files.readString(out, StandardCharsets.ISO_8859_1);
            Files.delete(out);
            return text;
        } catch (IOException e) {
            throw new IllegalStateException(e);
        }
    }

    static byte[] gradient(int w, int h, int channels) {
        byte[] b = new byte[w * h * channels];
        for (int i = 0; i < b.length; i++) {
            b[i] = (byte) (i * 7);
        }
        return b;
    }

    /**
     * Minimal valid baseline grayscale JPEG (flat mid-gray); w and h multiples of 8. One 8x8 block is a DC code "0"
     * plus an EOB code "0", each a 1-bit Huffman code. Port of {@code gray_jpeg} in core/tests/pdf.rs.
     */
    static byte[] grayJpeg(int w, int h) {
        int bits = (w / 8) * (h / 8) * 2;
        byte[] data = new byte[(bits + 7) / 8];
        int pad = data.length * 8 - bits;
        if (pad > 0) {
            data[data.length - 1] |= (byte) ((1 << pad) - 1);
        }
        ByteBuffer out = ByteBuffer.allocate(2 + 69 + 14 + 2 * 36 + 10 + data.length + 2);
        out.put(new byte[] {(byte) 0xff, (byte) 0xd8, (byte) 0xff, (byte) 0xdb, 0x00, 0x43, 0x00});
        for (int i = 0; i < 64; i++) {
            out.put((byte) 1);
        }
        out.put(new byte[] {(byte) 0xff, (byte) 0xc0, 0x00, 0x0b, 0x08});
        out.putShort((short) h).putShort((short) w).put(new byte[] {0x01, 0x01, 0x11, 0x00});
        for (int classId : new int[] {0x00, 0x10}) {
            out.put(new byte[] {(byte) 0xff, (byte) 0xc4, 0x00, 0x14, (byte) classId, 0x01});
            out.put(new byte[16]);
            out.put((byte) 0x00);
        }
        out.put(new byte[] {(byte) 0xff, (byte) 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00});
        out.put(data).put(new byte[] {(byte) 0xff, (byte) 0xd9});
        return java.util.Arrays.copyOf(out.array(), out.position());
    }

    static byte[] render(Consumer<PdfDocument> build) {
        try (PdfDocument doc = new PdfDocument()) {
            build.accept(doc);
            return doc.render();
        }
    }

    static boolean contains(byte[] haystack, byte[] needle) {
        return new String(haystack, StandardCharsets.ISO_8859_1)
                .contains(new String(needle, StandardCharsets.ISO_8859_1));
    }
}
