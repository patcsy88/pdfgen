package io.github.patcsy88.pdfgen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.ValueLayout;
import java.lang.ref.Cleaner;

/** Owns one native handle; a {@link Cleaner} frees it if {@link #close()} is never called. */
abstract class NativeHandle implements AutoCloseable {
    private static final Cleaner CLEANER = Cleaner.create();

    /** Frees a handle. Must not reference the owning object, or it would never become unreachable. */
    private record Releaser(MemorySegment handle, boolean table) implements Runnable {
        @Override
        public void run() {
            if (table) {
                Native.get().tableFree(handle);
            } else {
                Native.get().docFree(handle);
            }
        }
    }

    private final String what;
    private final MemorySegment handle;
    private final Cleaner.Cleanable cleanable;
    private boolean closed;

    NativeHandle(String what, MemorySegment handle, boolean table) {
        this.what = what;
        this.handle = handle;
        this.cleanable = CLEANER.register(this, new Releaser(handle, table));
    }

    final MemorySegment handle() {
        if (closed) {
            throw new IllegalStateException(what + " is closed");
        }
        return handle;
    }

    /** Frees the native object. Safe to call more than once. */
    @Override
    public final void close() {
        closed = true;
        cleanable.clean();
    }

    // Helpers shared by the public classes.

    /** UTF-8, NUL-terminated copy of {@code s} in {@code arena}; a NUL inside {@code s} is rejected. */
    static MemorySegment utf8(Arena arena, String s, String name) {
        if (s == null) {
            throw new NullPointerException(name + " must not be null");
        }
        if (s.indexOf('\0') >= 0) {
            // The C side would silently stop reading at the NUL.
            throw new IllegalArgumentException(name + " must not contain NUL characters");
        }
        return arena.allocateFrom(s);
    }

    /** A C array of pointers to NUL-terminated copies of {@code cells}. */
    static MemorySegment cellArray(Arena arena, String[] cells) {
        if (cells == null) {
            throw new NullPointerException("cells must not be null");
        }
        MemorySegment array = arena.allocate(MemoryLayout.sequenceLayout(cells.length, ValueLayout.ADDRESS));
        for (int i = 0; i < cells.length; i++) {
            array.setAtIndex(ValueLayout.ADDRESS, i, utf8(arena, cells[i], "cells[" + i + "]"));
        }
        return array;
    }

    /** The ABI takes unsigned 32-bit values; a negative Java int would wrap to a huge one. */
    static int unsigned(int value, String name) {
        if (value < 0) {
            throw new IllegalArgumentException(name + " must not be negative");
        }
        return value;
    }
}
