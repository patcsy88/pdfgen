using System.Runtime.InteropServices;

// Load the native library only from the directory of this assembly: the default probing also walks
// LD_LIBRARY_PATH / PATH, where a planted library of the same name would be loaded and run.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]

namespace PdfGen;

/// <summary>Raw P/Invoke declarations for the pdfgen_ffi C ABI. Use <see cref="PdfDocument"/> instead.</summary>
internal static partial class Native
{
    // Resolves to libpdfgen_ffi.so / libpdfgen_ffi.dylib / pdfgen_ffi.dll.
    private const string Lib = "pdfgen_ffi";

    internal const int Ok = 0;

    // The C ABI is cdecl; 32-bit Windows would otherwise default to stdcall and corrupt the stack.
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern IntPtr pdfgen_last_error();
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern IntPtr pdfgen_version();

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern IntPtr pdfgen_doc_new();
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern void pdfgen_doc_free(IntPtr doc);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pdfgen_doc_add_text(IntPtr doc, IntPtr utf8Text, int font, float size);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern int pdfgen_doc_add_page_break(IntPtr doc);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pdfgen_doc_add_image_raw(
        IntPtr doc, uint width, uint height, uint channels, byte[] data, UIntPtr len, float widthPt);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pdfgen_doc_add_image_jpeg(IntPtr doc, byte[] data, UIntPtr len, float widthPt);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern IntPtr pdfgen_table_new(float[] widths, UIntPtr ncols, float fontSize);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern void pdfgen_table_free(IntPtr table);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pdfgen_table_set_header(IntPtr table, IntPtr[] cells, UIntPtr n);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pdfgen_table_add_row(IntPtr table, IntPtr[] cells, UIntPtr n);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern int pdfgen_doc_add_table(IntPtr doc, IntPtr table);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pdfgen_doc_render(IntPtr doc, out IntPtr data, out UIntPtr len);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)] internal static extern void pdfgen_buffer_free(IntPtr data, UIntPtr len);
}
