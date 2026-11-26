package io.github.patcsy88.pdfgen;

/** The four built-in faces. {@code code} matches the C ABI font codes. */
public enum Font {
    HELVETICA(0),
    HELVETICA_BOLD(1),
    COURIER(2),
    COURIER_BOLD(3);

    final int code;

    Font(int code) {
        this.code = code;
    }
}
