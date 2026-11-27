package pdfgen_test

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	pdfgen "github.com/patcsy88/pdfgen/go"
)

func TestRawRGBAndGrayPixelsBecomeFlateImageXObjects(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		must(t, d.AddRawImage(16, 16, 3, gradient(16, 16, 3), 100))
		must(t, d.AddRawImage(8, 8, 1, gradient(8, 8, 1), 50))
	})
	text := qdf(t, checked(t, "images.pdf", pdf))
	for _, want := range []string{"/Subtype /Image", "/ColorSpace /DeviceRGB", "/ColorSpace /DeviceGray", "/Width 16"} {
		contains(t, text, want)
	}
}

func TestJPEGIsEmbeddedUnchangedAndQpdfCanDecodeIt(t *testing.T) {
	jpeg := grayJPEG(64, 32)
	path := checked(t, "jpeg.pdf", render(t, func(d *pdfgen.Document) { must(t, d.AddJPEG(jpeg, 128)) }))
	raw, err := os.ReadFile(path)
	must(t, err)
	if !bytes.Contains(raw, jpeg) {
		t.Error("jpeg bytes not embedded verbatim")
	}
	for _, want := range []string{"/Filter /DCTDecode", "/ColorSpace /DeviceGray", "/Width 64 /Height 32"} {
		contains(t, string(raw), want)
	}
}

func TestInvalidImagesCarryTheNativeMessage(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	defer d.Close()
	wantError(t, d.AddRawImage(4, 4, 2, make([]byte, 32), 40), -3, "channels")
	wantError(t, d.AddRawImage(4, 4, 3, make([]byte, 5), 40), -4, "")
	wantError(t, d.AddRawImage(4, 4, 1, make([]byte, 16), 0), -3, "")
	wantError(t, d.AddRawImage(4, 4, 1, nil, 40), -4, "")
	wantError(t, d.AddJPEG([]byte("not a jpeg"), 40), -4, "")
}

func TestExplicitPageBreaksStartNewPages(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		must(t, d.AddText("page one", pdfgen.Helvetica, 12))
		must(t, d.AddPageBreak())
		must(t, d.AddText("page two", pdfgen.Helvetica, 12))
		must(t, d.AddPageBreak())
		must(t, d.AddText("page three", pdfgen.Helvetica, 12))
	})
	if n := pageCount(t, checked(t, "breaks.pdf", pdf)); n != 3 {
		t.Errorf("pages = %d, want 3", n)
	}
}

func TestMixedReportOverSeveralPages(t *testing.T) {
	jpeg := grayJPEG(32, 32)
	pdf := render(t, func(d *pdfgen.Document) {
		must(t, d.AddText("Quarterly report", pdfgen.HelveticaBold, 20))
		must(t, d.AddText("Summary text.", pdfgen.Helvetica, 11))
		tb, err := pdfgen.NewTable([]float32{140, 90, 90}, 10)
		must(t, err)
		defer tb.Close()
		must(t, tb.SetHeader("Region", "Units", "Revenue"))
		for i := range 60 {
			must(t, tb.AddRow(fmt.Sprintf("Region %d", i), fmt.Sprint(i*3), fmt.Sprintf("%.2f", float64(i)*12.5)))
		}
		must(t, d.AddTable(tb))
		must(t, d.AddPageBreak())
		must(t, d.AddRawImage(16, 16, 3, gradient(16, 16, 3), 120))
		must(t, d.AddJPEG(jpeg, 80))
		must(t, d.AddText("x = 1", pdfgen.Courier, 9))
	})
	path := checked(t, "report.pdf", pdf)
	if n := pageCount(t, path); n < 3 {
		t.Errorf("pages = %d, want at least 3", n)
	}
	text := qdf(t, path)
	contains(t, text, "/BaseFont /Helvetica-Bold")
	contains(t, text, "/Subtype /Image")
}

func TestRenderTwiceGivesIdenticalBytesAndSaveMatches(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	defer d.Close()
	must(t, d.AddText("twice", pdfgen.Helvetica, 12))
	a, err := d.Render()
	must(t, err)
	b, err := d.Render()
	must(t, err)
	if !bytes.Equal(a, b) {
		t.Error("renders differ")
	}
	path := filepath.Join(outDir, "saved.pdf")
	must(t, d.Save(path))
	saved, err := os.ReadFile(path)
	must(t, err)
	if !bytes.Equal(saved, a) {
		t.Error("Save wrote different bytes than Render")
	}
	checked(t, "saved_check.pdf", saved)
}

func TestClosedObjectsReturnErrClosedAndDoubleCloseIsSafe(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	d.Close()
	d.Close()
	if err := d.AddText("x", pdfgen.Helvetica, 12); err != pdfgen.ErrClosed {
		t.Errorf("AddText on closed document: %v", err)
	}
	if _, err := d.Render(); err != pdfgen.ErrClosed {
		t.Errorf("Render on closed document: %v", err)
	}
	tb, err := pdfgen.NewTable([]float32{10}, 10)
	must(t, err)
	tb.Close()
	if err := tb.AddRow("x"); err != pdfgen.ErrClosed {
		t.Errorf("AddRow on closed table: %v", err)
	}
	other, err := pdfgen.NewDocument()
	must(t, err)
	defer other.Close()
	if err := other.AddTable(tb); err != pdfgen.ErrClosed {
		t.Errorf("AddTable with closed table: %v", err)
	}
	if err := other.AddTable(nil); err != pdfgen.ErrClosed {
		t.Errorf("AddTable(nil): %v", err)
	}
}

// Copying the struct copies a handle reference, not a second owner: closing either copy closes both and
// the native object is freed once.
func TestCopiedDocumentAndTableShareOneHandle(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	cp := *d
	cp.Close()
	if err := d.AddText("x", pdfgen.Helvetica, 12); err != pdfgen.ErrClosed {
		t.Errorf("original after closing its copy: %v", err)
	}
	d.Close()

	tb, err := pdfgen.NewTable([]float32{10}, 10)
	must(t, err)
	tcp := *tb
	tb.Close()
	if err := tcp.AddRow("x"); err != pdfgen.ErrClosed {
		t.Errorf("table copy after closing the original: %v", err)
	}
	tcp.Close()
}

func TestConcurrentCloseFreesTheHandleOnce(t *testing.T) {
	for range 50 {
		d, err := pdfgen.NewDocument()
		must(t, err)
		var wg sync.WaitGroup
		for range 8 {
			wg.Add(1)
			go func() { defer wg.Done(); d.Close() }()
		}
		wg.Wait()
		if _, err := d.Render(); err != pdfgen.ErrClosed {
			t.Fatalf("Render after concurrent Close: %v", err)
		}
	}
}

// Close from another goroutine waits for a Render in flight instead of freeing the handle under it. The
// worker renders in a loop and Close starts only after its first render succeeded, so later renders are
// very likely in flight when Close runs; each must return a valid PDF or ErrClosed.
func TestCloseDuringRenderWaitsForTheRender(t *testing.T) {
	text := strings.Repeat("lorem ipsum dolor sit amet ", 20000)
	for range 5 {
		d, err := pdfgen.NewDocument()
		must(t, err)
		must(t, d.AddText(text, pdfgen.Helvetica, 12))
		started := make(chan struct{})
		done := make(chan error, 1)
		go func() {
			for i := 0; ; i++ {
				pdf, err := d.Render()
				if err == pdfgen.ErrClosed {
					done <- nil
					return
				}
				if err != nil {
					done <- err
					return
				}
				if !bytes.HasPrefix(pdf, []byte("%PDF-2.0")) {
					done <- fmt.Errorf("header %q", pdf[:min(len(pdf), 8)])
					return
				}
				if i == 0 {
					close(started)
				}
			}
		}()
		select {
		case <-started:
		case err := <-done:
			t.Fatalf("worker stopped before its first render finished: %v", err)
		}
		d.Close()
		if err := <-done; err != nil {
			t.Fatal(err)
		}
	}
}

func TestManyIndependentDocuments(t *testing.T) {
	for i := range 200 {
		pdf := render(t, func(d *pdfgen.Document) { must(t, d.AddText(fmt.Sprintf("doc %d", i), pdfgen.Courier, 10)) })
		if !bytes.HasPrefix(pdf, []byte("%PDF-")) {
			t.Fatalf("doc %d: header %q", i, pdf[:5])
		}
	}
}

// Each goroutine reads its own error message even though they all share the library's thread-local slot,
// because every failing call and its message read stay on one locked OS thread.
func TestErrorMessagesDoNotLeakBetweenGoroutines(t *testing.T) {
	var wg sync.WaitGroup
	msgs := make([]string, 16)
	for i := range msgs {
		wg.Add(1)
		go func() {
			defer wg.Done()
			d, err := pdfgen.NewDocument()
			if err != nil {
				msgs[i] = err.Error()
				return
			}
			defer d.Close()
			for range 200 {
				err = d.AddText("x", pdfgen.Font(100+i), 12)
			}
			if pe, ok := err.(*pdfgen.Error); ok {
				msgs[i] = pe.Message
			}
		}()
	}
	wg.Wait()
	for i, m := range msgs {
		if m != "font code out of range" {
			t.Errorf("goroutine %d got %q", i, m)
		}
	}
}

func TestManyDocumentsOnManyGoroutinesRenderValidPDFs(t *testing.T) {
	var wg sync.WaitGroup
	var mu sync.Mutex
	var bad []string
	count := 0
	for g := range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := range 25 {
				d, err := pdfgen.NewDocument()
				if err != nil {
					mu.Lock()
					bad = append(bad, err.Error())
					mu.Unlock()
					return
				}
				err = d.AddText(fmt.Sprintf("goroutine %d doc %d", g, j), pdfgen.Helvetica, 12)
				var pdf []byte
				if err == nil {
					pdf, err = d.Render()
				}
				d.Close()
				mu.Lock()
				count++
				if err != nil || !strings.HasPrefix(string(pdf), "%PDF-2.0") {
					bad = append(bad, fmt.Sprintf("g%d j%d: %v", g, j, err))
				}
				mu.Unlock()
			}
		}()
	}
	wg.Wait()
	if count != 200 || len(bad) > 0 {
		t.Errorf("count = %d, failures = %v", count, bad)
	}
}
