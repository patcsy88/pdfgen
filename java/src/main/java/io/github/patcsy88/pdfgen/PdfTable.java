package io.github.patcsy88.pdfgen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/** A table under construction. Close it after adding it to a document. Not thread-safe. */
public final class PdfTable extends NativeHandle {
    /** A table with the given column widths in points and the default 10 pt font. */
    public PdfTable(float[] columnWidths) {
        this(columnWidths, 10f);
    }

    public PdfTable(float[] columnWidths, float fontSize) {
        super("PdfTable", create(fontSize, columnWidths), true);
    }

    private static MemorySegment create(float fontSize, float[] widths) {
        if (widths == null) {
            throw new NullPointerException("columnWidths must not be null");
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment handle = Native.get().tableNew(arena.allocateFrom(ValueLayout.JAVA_FLOAT, widths),
                    widths.length, fontSize);
            if (handle.equals(MemorySegment.NULL)) {
                throw new PdfException(Native.ERR_ARG, Native.get().errorMessage());
            }
            return handle;
        }
    }

    public PdfTable setHeader(String... cells) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment array = cellArray(arena, cells);
            try (Hold h = hold()) {
                PdfException.check(Native.get().tableSetHeader(h.handle, array, cells.length));
            }
        }
        return this;
    }

    public PdfTable addRow(String... cells) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment array = cellArray(arena, cells);
            try (Hold h = hold()) {
                PdfException.check(Native.get().tableAddRow(h.handle, array, cells.length));
            }
        }
        return this;
    }
}
