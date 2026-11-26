using System.Runtime.InteropServices;

namespace PdfGen;

/// <summary>The four built-in faces. Values match the C ABI font codes.</summary>
public enum PdfFont
{
    Helvetica = 0,
    HelveticaBold = 1,
    Courier = 2,
    CourierBold = 3,
}

/// <summary>Raised when the native library reports an error.</summary>
public sealed class PdfException : Exception
{
    /// <summary>Negative PDFGEN_ERR_* code from the C ABI.</summary>
    public int Code { get; }

    public PdfException(int code, string message) : base(message) => Code = code;
}

internal static class Interop
{
    internal static void Check(int rc)
    {
        if (rc == Native.Ok) return;
        throw new PdfException(rc, LastError());
    }

    internal static string LastError() =>
        Marshal.PtrToStringUTF8(Native.pdfgen_last_error()) ?? "";

    /// <summary>Runs <paramref name="use"/> with NUL-terminated UTF-8 copies of <paramref name="cells"/>.</summary>
    internal static int WithUtf8(IReadOnlyList<string> cells, Func<IntPtr[], int> use)
    {
        var ptrs = new IntPtr[cells.Count];
        try
        {
            for (int i = 0; i < ptrs.Length; i++)
                ptrs[i] = Marshal.StringToCoTaskMemUTF8(cells[i] ?? throw new ArgumentNullException(nameof(cells)));
            return use(ptrs);
        }
        finally
        {
            foreach (var p in ptrs) if (p != IntPtr.Zero) Marshal.FreeCoTaskMem(p);
        }
    }
}

/// <summary>A table under construction. Dispose it after adding it to a document.</summary>
public sealed class PdfTable : IDisposable
{
    private IntPtr _handle;

    public PdfTable(IReadOnlyList<float> columnWidths, float fontSize = 10f)
    {
        ArgumentNullException.ThrowIfNull(columnWidths);
        var w = columnWidths.ToArray();
        _handle = Native.pdfgen_table_new(w, (UIntPtr)w.Length, fontSize);
        if (_handle == IntPtr.Zero) throw new PdfException(-3, Interop.LastError());
    }

    internal IntPtr Handle => _handle != IntPtr.Zero ? _handle : throw new ObjectDisposedException(nameof(PdfTable));

    public PdfTable SetHeader(params string[] cells)
    {
        var h = Handle;
        Interop.Check(Interop.WithUtf8(cells, p => Native.pdfgen_table_set_header(h, p, (UIntPtr)p.Length)));
        return this;
    }

    public PdfTable AddRow(params string[] cells)
    {
        var h = Handle;
        Interop.Check(Interop.WithUtf8(cells, p => Native.pdfgen_table_add_row(h, p, (UIntPtr)p.Length)));
        return this;
    }

    public void Dispose()
    {
        var h = Interlocked.Exchange(ref _handle, IntPtr.Zero);
        if (h != IntPtr.Zero) Native.pdfgen_table_free(h);
        GC.SuppressFinalize(this);
    }

    ~PdfTable() => Dispose();
}

/// <summary>A document being built. Not thread-safe; use one instance per thread.</summary>
public sealed class PdfDocument : IDisposable
{
    private IntPtr _handle;

    public PdfDocument()
    {
        _handle = Native.pdfgen_doc_new();
        if (_handle == IntPtr.Zero) throw new PdfException(-4, "pdfgen_doc_new returned null");
    }

    /// <summary>Version of the native library.</summary>
    public static string NativeVersion => Marshal.PtrToStringUTF8(Native.pdfgen_version()) ?? "";

    private IntPtr Handle => _handle != IntPtr.Zero ? _handle : throw new ObjectDisposedException(nameof(PdfDocument));

    public PdfDocument AddText(string text, PdfFont font = PdfFont.Helvetica, float size = 12f)
    {
        ArgumentNullException.ThrowIfNull(text);
        var h = Handle;
        var p = Marshal.StringToCoTaskMemUTF8(text);
        try { Interop.Check(Native.pdfgen_doc_add_text(h, p, (int)font, size)); }
        finally { Marshal.FreeCoTaskMem(p); }
        return this;
    }

    public PdfDocument AddPageBreak()
    {
        Interop.Check(Native.pdfgen_doc_add_page_break(Handle));
        return this;
    }

    /// <summary>Adds 8-bit RGB (<paramref name="channels"/> = 3) or gray (1) pixels, drawn <paramref name="widthPt"/> points wide.</summary>
    public PdfDocument AddRawImage(int width, int height, int channels, byte[] pixels, float widthPt)
    {
        ArgumentNullException.ThrowIfNull(pixels);
        if (width < 0 || height < 0 || channels < 0) throw new ArgumentOutOfRangeException(nameof(width));
        Interop.Check(Native.pdfgen_doc_add_image_raw(
            Handle, (uint)width, (uint)height, (uint)channels, pixels, (UIntPtr)pixels.Length, widthPt));
        return this;
    }

    public PdfDocument AddJpeg(byte[] jpeg, float widthPt)
    {
        ArgumentNullException.ThrowIfNull(jpeg);
        Interop.Check(Native.pdfgen_doc_add_image_jpeg(Handle, jpeg, (UIntPtr)jpeg.Length, widthPt));
        return this;
    }

    /// <summary>Copies the table into the document; the caller still owns <paramref name="table"/>.</summary>
    public PdfDocument AddTable(PdfTable table)
    {
        ArgumentNullException.ThrowIfNull(table);
        Interop.Check(Native.pdfgen_doc_add_table(Handle, table.Handle));
        GC.KeepAlive(table);
        return this;
    }

    /// <summary>Lays out the document and returns the PDF bytes.</summary>
    public byte[] Render()
    {
        Interop.Check(Native.pdfgen_doc_render(Handle, out var data, out var len));
        GC.KeepAlive(this);
        try
        {
            var bytes = new byte[checked((int)len)];
            Marshal.Copy(data, bytes, 0, bytes.Length);
            return bytes;
        }
        finally { Native.pdfgen_buffer_free(data, len); }
    }

    public void Save(string path) => File.WriteAllBytes(path, Render());

    public void Dispose()
    {
        var h = Interlocked.Exchange(ref _handle, IntPtr.Zero);
        if (h != IntPtr.Zero) Native.pdfgen_doc_free(h);
        GC.SuppressFinalize(this);
    }

    ~PdfDocument() => Dispose();
}
