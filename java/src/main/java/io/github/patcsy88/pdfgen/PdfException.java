package io.github.patcsy88.pdfgen;

/** The native library reported an error. {@link #code()} is the negative PDFGEN_ERR_* value. */
public final class PdfException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    private final int code;

    public PdfException(int code, String message) {
        super(message);
        this.code = code;
    }

    public int code() {
        return code;
    }

    /** Throws if {@code rc} is not success, using the calling thread's native error message. */
    static void check(int rc) {
        if (rc != Native.OK) {
            throw new PdfException(rc, Native.get().errorMessage());
        }
    }
}
