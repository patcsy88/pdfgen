using System.Diagnostics;
using System.Text;
using PdfGen;
using Xunit;

namespace PdfGen.Tests;

public class PdfTests
{
    // Validity check, same as the Rust tests: `qpdf --check` must exit 0.
    // qpdf is not a PDF 2.0 conformance validator; see core/tests/pdf.rs for the
    // clause-level checks and what they do not cover.
    private static (int Exit, string Output) Qpdf(params string[] args)
    {
        var psi = new ProcessStartInfo("qpdf")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        foreach (var a in args) psi.ArgumentList.Add(a);
        using var p = Process.Start(psi) ?? throw new InvalidOperationException("could not start qpdf");
        var stderr = p.StandardError.ReadToEndAsync();
        var stdout = p.StandardOutput.ReadToEnd();
        p.WaitForExit();
        return (p.ExitCode, stdout + stderr.Result);
    }

    private static string CheckValid(string name, byte[] pdf)
    {
        var path = Path.Combine(AppContext.BaseDirectory, name);
        File.WriteAllBytes(path, pdf);
        var (exit, output) = Qpdf("--check", path);
        Assert.True(exit == 0, $"qpdf --check exited {exit}:\n{output}");
        Assert.DoesNotContain("WARNING", output);
        return path;
    }

    private static int PageCount(string path)
    {
        var (exit, output) = Qpdf("--show-npages", path);
        Assert.Equal(0, exit);
        return int.Parse(output.Trim());
    }

    [Fact]
    public void NativeVersionIsReported()
    {
        Assert.Matches(@"^\d+\.\d+\.\d+", PdfDocument.NativeVersion);
    }

    [Fact]
    public void FullReportIsValidPdf()
    {
        using var doc = new PdfDocument();
        doc.AddText("Quarterly Report", PdfFont.HelveticaBold, 20f)
           .AddText("caf\u00e9 (naïve) \u2603 snowman falls back to ?", PdfFont.Helvetica, 11f)
           .AddText("fixed width", PdfFont.Courier, 10f);

        using (var table = new PdfTable(new[] { 60f, 200f, 100f }, 10f))
        {
            table.SetHeader("ID", "Description", "Amount");
            for (int i = 0; i < 90; i++)
                table.AddRow($"R{i:000}", $"Line item {i} with a rather long description", $"{i * 7}.00");
            doc.AddTable(table);
        }

        var px = new byte[16 * 16 * 3];
        for (int i = 0; i < px.Length; i++) px[i] = (byte)(i * 3);
        doc.AddRawImage(16, 16, 3, px, 120f)
           .AddPageBreak()
           .AddText("last page", PdfFont.CourierBold, 12f);

        var pdf = doc.Render();
        Assert.StartsWith("%PDF-2.0", Encoding.ASCII.GetString(pdf, 0, 8));
        Assert.EndsWith("%%EOF\n", Encoding.ASCII.GetString(pdf, pdf.Length - 6, 6));
        var path = CheckValid("dotnet_report.pdf", pdf);
        Assert.True(PageCount(path) >= 3);
    }

    [Fact]
    public void SaveWritesTheSameBytesAsRender()
    {
        using var doc = new PdfDocument();
        doc.AddText("hello");
        var path = Path.Combine(AppContext.BaseDirectory, "dotnet_save.pdf");
        doc.Save(path);
        var saved = File.ReadAllBytes(path);
        Assert.Equal(doc.Render(), saved);
        CheckValid("dotnet_save_check.pdf", saved);
    }

    [Fact]
    public void EmptyDocumentIsValid()
    {
        using var doc = new PdfDocument();
        var path = CheckValid("dotnet_empty.pdf", doc.Render());
        Assert.Equal(1, PageCount(path));
    }

    [Fact]
    public void ErrorsSurfaceAsPdfExceptionWithNativeMessage()
    {
        using var doc = new PdfDocument();
        var ex = Assert.Throws<PdfException>(() => doc.AddRawImage(2, 2, 3, new byte[5], 50f));
        Assert.Equal(-4, ex.Code);
        Assert.Contains("expected 12 bytes", ex.Message);

        ex = Assert.Throws<PdfException>(() => doc.AddRawImage(2, 2, 4, new byte[16], 50f));
        Assert.Equal(-3, ex.Code);

        ex = Assert.Throws<PdfException>(() => doc.AddText("x", (PdfFont)9));
        Assert.Equal(-3, ex.Code);

        ex = Assert.Throws<PdfException>(() => doc.AddJpeg(new byte[] { 1, 2, 3, 4 }, 50f));
        Assert.Equal(-4, ex.Code);

        // Too many cells for the table is detected at render time.
        using var table = new PdfTable(new[] { 50f });
        table.AddRow("a", "b");
        doc.AddTable(table);
        ex = Assert.Throws<PdfException>(() => doc.Render());
        Assert.Contains("more cells than columns", ex.Message);
    }

    [Fact]
    public void InvalidTableArgumentsAreRejected()
    {
        Assert.Throws<PdfException>(() => new PdfTable(Array.Empty<float>()));
        Assert.Throws<PdfException>(() => new PdfTable(new[] { 50f }, -1f));
    }

    [Fact]
    public void DisposedObjectsThrowAndDoubleDisposeIsSafe()
    {
        var doc = new PdfDocument();
        doc.Dispose();
        doc.Dispose();
        Assert.Throws<ObjectDisposedException>(() => doc.AddText("x"));
        Assert.Throws<ObjectDisposedException>(() => doc.Render());

        var table = new PdfTable(new[] { 50f });
        table.Dispose();
        table.Dispose();
        Assert.Throws<ObjectDisposedException>(() => table.AddRow("x"));
    }

    [Fact]
    public async Task ManyDocumentsOnManyThreads()
    {
        var tasks = Enumerable.Range(0, 8).Select(t => Task.Run(() =>
        {
            for (int i = 0; i < 20; i++)
            {
                using var doc = new PdfDocument();
                doc.AddText($"thread {t} doc {i}");
                var pdf = doc.Render();
                Assert.StartsWith("%PDF-2.0", Encoding.ASCII.GetString(pdf, 0, 8));
            }
        })).ToArray();
        await Task.WhenAll(tasks);
    }

    // Render/AddTable must keep their managed owners alive across the native call;
    // otherwise the finalizer can free the handle while native code is using it.
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static byte[] RenderUnowned(string text) => new PdfDocument().AddText(text).Render();

    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static PdfDocument AddUnownedTable(PdfDocument doc, string[] row)
    {
        var table = new PdfTable(new[] { 100f, 100f, 100f });
        for (int i = 0; i < 200; i++) table.AddRow(row);
        return doc.AddTable(table);
    }

    [Fact]
    public async Task UndisposedObjectsSurviveGcDuringNativeCalls()
    {
        var text = string.Concat(Enumerable.Repeat("lorem ipsum dolor sit amet ", 4000));
        var reference = new PdfDocument().AddText(text).Render();
        var row = new[] { "a", "b", "c" };
        using var stop = new CancellationTokenSource(TimeSpan.FromSeconds(2.5));
        var gc = Task.Run(() =>
        {
            while (!stop.IsCancellationRequested) { GC.Collect(); GC.WaitForPendingFinalizers(); }
        });
        var workers = Enumerable.Range(0, 4).Select(_ => Task.Run(() =>
        {
            while (!stop.IsCancellationRequested)
            {
                Assert.Equal(reference, RenderUnowned(text));
                using var doc = new PdfDocument();
                AddUnownedTable(doc, row);
            }
        })).ToArray();
        await Task.WhenAll(workers);
        await gc;
    }
}
