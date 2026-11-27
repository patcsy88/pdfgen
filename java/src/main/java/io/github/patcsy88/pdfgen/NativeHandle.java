package io.github.patcsy88.pdfgen;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.ValueLayout;
import java.lang.ref.Cleaner;
import java.util.concurrent.locks.ReentrantLock;

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
    private final ReentrantLock lock = new ReentrantLock();
    private boolean closed; // guarded by lock

    NativeHandle(String what, MemorySegment handle, boolean table) {
        this.what = what;
        this.handle = handle;
        this.cleanable = CLEANER.register(this, new Releaser(handle, table));
    }

    /**
     * The handle, locked against {@link #close()} until this is closed. Use in try-with-resources around
     * every native call. The unlock also keeps the owner reachable, so the Cleaner cannot free the handle
     * mid-call. When two objects are held, always hold the document first, then the table.
     */
    final class Hold implements AutoCloseable {
        final MemorySegment handle;

        private Hold(MemorySegment handle) {
            this.handle = handle;
        }

        @Override
        public void close() {
            lock.unlock();
        }
    }

    final Hold hold() {
        lock.lock();
        if (closed) {
            lock.unlock();
            throw new IllegalStateException(what + " is closed");
        }
        return new Hold(handle);
    }

    /** Frees the native object. Safe to call more than once and from several threads; waits for a call in flight. */
    @Override
    public final void close() {
        lock.lock();
        try {
            closed = true;
            cleanable.clean();
        } finally {
            lock.unlock();
        }
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
