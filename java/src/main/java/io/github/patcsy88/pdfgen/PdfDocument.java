package io.github.patcsy88.pdfgen;

import java.io.IOException;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.file.Files;
import java.nio.file.Path;

/** A document being built. Close it when done. Not thread-safe: use one instance per thread. */
public final class PdfDocument extends NativeHandle {
    public PdfDocument() {
        super("PdfDocument", create(), false);
    }

    private static MemorySegment create() {
        MemorySegment handle = Native.get().docNew();
        if (handle.equals(MemorySegment.NULL)) {
            throw new PdfException(Native.ERR_CORE, "pdfgen_doc_new returned null");
        }
        return handle;
    }

    /** Version of the native library. */
    public static String nativeVersion() {
        return Native.get().versionString();
    }

    public PdfDocument addText(String text, Font font, float size) {
        if (font == null) {
            throw new NullPointerException("font must not be null");
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment utf8 = utf8(arena, text, "text");
            try (Hold h = hold()) {
                PdfException.check(Native.get().docAddText(h.handle, utf8, font.code, size));
            }
        }
        return this;
    }

    /** Adds 12 pt Helvetica text. */
    public PdfDocument addText(String text) {
        return addText(text, Font.HELVETICA, 12f);
    }

    public PdfDocument addPageBreak() {
        try (Hold h = hold()) {
            PdfException.check(Native.get().docAddPageBreak(h.handle));
        }
        return this;
    }

    /** Adds 8-bit RGB ({@code channels} = 3) or gray (1) pixels, drawn {@code widthPt} points wide. */
    public PdfDocument addRawImage(int width, int height, int channels, byte[] pixels, float widthPt) {
        unsigned(width, "width");
        unsigned(height, "height");
        unsigned(channels, "channels");
        if (pixels == null) {
            throw new NullPointerException("pixels must not be null");
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment data = arena.allocateFrom(ValueLayout.JAVA_BYTE, pixels);
            try (Hold h = hold()) {
                PdfException.check(
                        Native.get().docAddImageRaw(h.handle, width, height, channels, data, pixels.length, widthPt));
            }
        }
        return this;
    }

    public PdfDocument addJpeg(byte[] jpeg, float widthPt) {
        if (jpeg == null) {
            throw new NullPointerException("jpeg must not be null");
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment data = arena.allocateFrom(ValueLayout.JAVA_BYTE, jpeg);
            try (Hold h = hold()) {
                PdfException.check(Native.get().docAddImageJpeg(h.handle, data, jpeg.length, widthPt));
            }
        }
        return this;
    }

    /** Copies the table into the document; the caller still owns {@code table}. */
    public PdfDocument addTable(PdfTable table) {
        if (table == null) {
            throw new NullPointerException("table must not be null");
        }
        try (Hold d = hold(); Hold t = table.hold()) {
            PdfException.check(Native.get().docAddTable(d.handle, t.handle));
        }
        return this;
    }

    /** Lays out the document and returns the PDF bytes. */
    public byte[] render() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment outData = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            try (Hold h = hold()) {
                PdfException.check(Native.get().docRender(h.handle, outData, outLen));
            }
            long len = outLen.get(ValueLayout.JAVA_LONG, 0);
            MemorySegment data = outData.get(ValueLayout.ADDRESS, 0);
            try {
                return data.reinterpret(len).toArray(ValueLayout.JAVA_BYTE);
            } finally {
                Native.get().bufferFree(data, len);
            }
        }
    }

    /** Renders the document and writes it to {@code path}. */
    public void save(Path path) throws IOException {
        Files.write(path, render());
    }
}
