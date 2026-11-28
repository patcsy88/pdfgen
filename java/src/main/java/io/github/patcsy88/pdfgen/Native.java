package io.github.patcsy88.pdfgen;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_FLOAT;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.nio.file.Files;
import java.nio.file.Path;

/** Downcall handles for the pdfgen_ffi C ABI. Package-private: use the public classes instead. */
final class Native {
    static final int OK = 0;
    static final int ERR_ARG = -3;
    static final int ERR_CORE = -4;

    private static volatile Native instance;

    /** Loads the library on first use. A load failure is thrown again on every call. */
    static Native get() {
        Native n = instance;
        if (n == null) {
            synchronized (Native.class) {
                n = instance;
                if (n == null) {
                    instance = n = new Native();
                }
            }
        }
        return n;
    }

    private static Path libraryPath() {
        // No search: a directory walk from the cwd would load whatever library sits in a parent directory.
        String override = System.getenv("PDFGEN_LIB");
        if (override == null || override.isEmpty()) {
            throw new UnsatisfiedLinkError("PDFGEN_LIB is not set; set it to the path of "
                    + System.mapLibraryName("pdfgen_ffi") + " (`cargo build --release -p ffi` builds it in target/release/)");
        }
        // Absolute, so the existence check and the load name the same file whatever a JDK does with a relative path.
        return Path.of(override).toAbsolutePath().normalize();
    }

    private final SymbolLookup lib;
    private final MethodHandle lastError;
    private final MethodHandle version;
    private final MethodHandle docNew;
    private final MethodHandle docFree;
    private final MethodHandle docAddText;
    private final MethodHandle docAddPageBreak;
    private final MethodHandle docAddImageRaw;
    private final MethodHandle docAddImageJpeg;
    private final MethodHandle tableNew;
    private final MethodHandle tableFree;
    private final MethodHandle tableSetHeader;
    private final MethodHandle tableAddRow;
    private final MethodHandle docAddTable;
    private final MethodHandle docRender;
    private final MethodHandle bufferFree;

    private Native() {
        Path path = libraryPath();
        if (!Files.isRegularFile(path)) {
            throw new UnsatisfiedLinkError("pdfgen native library not found at " + path
                    + "; run `cargo build --release -p ffi` or set PDFGEN_LIB");
        }
        lib = SymbolLookup.libraryLookup(path, Arena.global());
        lastError = handle("pdfgen_last_error", FunctionDescriptor.of(ADDRESS));
        version = handle("pdfgen_version", FunctionDescriptor.of(ADDRESS));
        docNew = handle("pdfgen_doc_new", FunctionDescriptor.of(ADDRESS));
        docFree = handle("pdfgen_doc_free", FunctionDescriptor.ofVoid(ADDRESS));
        docAddText = handle("pdfgen_doc_add_text",
                FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, JAVA_INT, JAVA_FLOAT));
        docAddPageBreak = handle("pdfgen_doc_add_page_break", FunctionDescriptor.of(JAVA_INT, ADDRESS));
        docAddImageRaw = handle("pdfgen_doc_add_image_raw", FunctionDescriptor.of(
                JAVA_INT, ADDRESS, JAVA_INT, JAVA_INT, JAVA_INT, ADDRESS, JAVA_LONG, JAVA_FLOAT));
        docAddImageJpeg = handle("pdfgen_doc_add_image_jpeg",
                FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG, JAVA_FLOAT));
        tableNew = handle("pdfgen_table_new", FunctionDescriptor.of(ADDRESS, ADDRESS, JAVA_LONG, JAVA_FLOAT));
        tableFree = handle("pdfgen_table_free", FunctionDescriptor.ofVoid(ADDRESS));
        tableSetHeader = handle("pdfgen_table_set_header",
                FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG));
        tableAddRow = handle("pdfgen_table_add_row",
                FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG));
        docAddTable = handle("pdfgen_doc_add_table", FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS));
        docRender = handle("pdfgen_doc_render", FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, ADDRESS));
        bufferFree = handle("pdfgen_buffer_free", FunctionDescriptor.ofVoid(ADDRESS, JAVA_LONG));
    }

    private MethodHandle handle(String name, FunctionDescriptor descriptor) {
        MemorySegment symbol = lib.find(name).orElseThrow(
                () -> new UnsatisfiedLinkError("symbol " + name + " missing from the pdfgen library"));
        return Linker.nativeLinker().downcallHandle(symbol, descriptor);
    }

    /** Always throws: Errors as they are, everything else as an unchecked exception. */
    private static RuntimeException rethrow(Throwable t) {
        if (t instanceof Error e) {
            throw e;
        }
        if (t instanceof RuntimeException e) {
            throw e;
        }
        throw new IllegalStateException(t);
    }

    private static String cString(MemorySegment p) {
        return p.equals(MemorySegment.NULL) ? "" : p.reinterpret(Long.MAX_VALUE).getString(0);
    }

    /** The calling thread's last native error message (thread-local in the library). */
    String errorMessage() {
        try {
            return cString((MemorySegment) lastError.invokeExact());
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    String versionString() {
        try {
            return cString((MemorySegment) version.invokeExact());
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    MemorySegment docNew() {
        try {
            return (MemorySegment) docNew.invokeExact();
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    void docFree(MemorySegment doc) {
        try {
            docFree.invokeExact(doc);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int docAddText(MemorySegment doc, MemorySegment text, int font, float size) {
        try {
            return (int) docAddText.invokeExact(doc, text, font, size);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int docAddPageBreak(MemorySegment doc) {
        try {
            return (int) docAddPageBreak.invokeExact(doc);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int docAddImageRaw(MemorySegment doc, int width, int height, int channels, MemorySegment data, long len,
            float widthPt) {
        try {
            return (int) docAddImageRaw.invokeExact(doc, width, height, channels, data, len, widthPt);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int docAddImageJpeg(MemorySegment doc, MemorySegment data, long len, float widthPt) {
        try {
            return (int) docAddImageJpeg.invokeExact(doc, data, len, widthPt);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    MemorySegment tableNew(MemorySegment widths, long count, float fontSize) {
        try {
            return (MemorySegment) tableNew.invokeExact(widths, count, fontSize);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    void tableFree(MemorySegment table) {
        try {
            tableFree.invokeExact(table);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int tableSetHeader(MemorySegment table, MemorySegment cells, long count) {
        try {
            return (int) tableSetHeader.invokeExact(table, cells, count);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int tableAddRow(MemorySegment table, MemorySegment cells, long count) {
        try {
            return (int) tableAddRow.invokeExact(table, cells, count);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int docAddTable(MemorySegment doc, MemorySegment table) {
        try {
            return (int) docAddTable.invokeExact(doc, table);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    int docRender(MemorySegment doc, MemorySegment outData, MemorySegment outLen) {
        try {
            return (int) docRender.invokeExact(doc, outData, outLen);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    void bufferFree(MemorySegment data, long len) {
        try {
            bufferFree.invokeExact(data, len);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }
}
