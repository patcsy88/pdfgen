package pdfgen_test

import (
	"fmt"
	"math"
	"strings"
	"testing"

	pdfgen "github.com/patcsy88/pdfgen/go"
)

func TestVersion(t *testing.T) {
	if v := pdfgen.Version(); !strings.Contains(v, ".") {
		t.Errorf("version %q", v)
	}
}

func TestTextLatin1AsPDF20(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		must(t, d.AddText("Hello from Go, caf\u00e9", pdfgen.Helvetica, 14))
	})
	if !strings.HasPrefix(string(pdf), "%PDF-2.0") {
		t.Errorf("header %q", pdf[:8])
	}
	path := checked(t, "text.pdf", pdf)
	contains(t, qdf(t, path), "/F1 14 Tf")
	if n := pageCount(t, path); n != 1 {
		t.Errorf("pages = %d, want 1", n)
	}
}

func TestLongTextFlowsOntoFurtherPages(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		for i := range 120 {
			must(t, d.AddText(fmt.Sprintf("Line %d: the quick brown fox jumps over the lazy dog.", i), pdfgen.Helvetica, 12))
		}
	})
	if n := pageCount(t, checked(t, "flow.pdf", pdf)); n < 2 {
		t.Errorf("pages = %d, want at least 2", n)
	}
}

func TestIdenticalInputGivesIdenticalBytes(t *testing.T) {
	build := func(d *pdfgen.Document) { must(t, d.AddText("same", pdfgen.Courier, 10)) }
	if a, b := render(t, build), render(t, build); string(a) != string(b) {
		t.Error("two renders of the same input differ")
	}
}

func TestEmptyDocumentIsValid(t *testing.T) {
	path := checked(t, "empty.pdf", render(t, func(*pdfgen.Document) {}))
	if n := pageCount(t, path); n != 1 {
		t.Errorf("pages = %d, want 1", n)
	}
}

func TestTextOutsideLatin1StillRenders(t *testing.T) {
	checked(t, "text_unicode.pdf", render(t, func(d *pdfgen.Document) {
		must(t, d.AddText("snowman \u2603 and kanji \u6f22", pdfgen.Helvetica, 12))
	}))
}

func TestAllFourFacesAreDeclaredAndUsed(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		must(t, d.AddText("sans", pdfgen.Helvetica, 12))
		must(t, d.AddText("sans bold", pdfgen.HelveticaBold, 12))
		must(t, d.AddText("mono", pdfgen.Courier, 12))
		must(t, d.AddText("mono bold", pdfgen.CourierBold, 12))
	})
	text := qdf(t, checked(t, "fonts.pdf", pdf))
	for _, base := range []string{"Helvetica", "Helvetica-Bold", "Courier", "Courier-Bold"} {
		contains(t, text, "/BaseFont /"+base)
	}
	for _, f := range []string{"F1", "F2", "F3", "F4"} {
		contains(t, text, "/"+f+" 12 Tf")
	}
}

func TestBadFontCodeAndSizesCarryTheNativeMessage(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	defer d.Close()
	wantError(t, d.AddText("x", pdfgen.Font(9), 12), -3, "font")
	for _, size := range []float32{0, -1, float32(math.NaN()), float32(math.Inf(1))} {
		wantError(t, d.AddText("x", pdfgen.Helvetica, size), -3, "")
	}
}

func TestTextWithNULIsRejectedBeforeTheCall(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	defer d.Close()
	if err := d.AddText("before\x00after", pdfgen.Helvetica, 12); err == nil || !strings.Contains(err.Error(), "NUL") {
		t.Errorf("err = %v", err)
	}
}

func sampleTable(t *testing.T) *pdfgen.Table {
	t.Helper()
	tb, err := pdfgen.NewTable([]float32{120, 80, 80}, 10)
	must(t, err)
	must(t, tb.SetHeader("Item", "Qty", "Price"))
	must(t, tb.AddRow("Widget", "4", "3.50"))
	must(t, tb.AddRow("Gadget", "12", "19.99"))
	return tb
}

func TestTableRendersHeaderCellsAndBorders(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		tb := sampleTable(t)
		defer tb.Close()
		must(t, d.AddTable(tb))
	})
	text := qdf(t, checked(t, "table.pdf", pdf))
	for _, cell := range []string{"Item", "Qty", "Price", "Widget", "Gadget", "19.99"} {
		contains(t, text, cell)
	}
}

func TestTableWrapsLongCellText(t *testing.T) {
	checked(t, "table_wrap.pdf", render(t, func(d *pdfgen.Document) {
		tb, err := pdfgen.NewTable([]float32{60, 60}, 10)
		must(t, err)
		defer tb.Close()
		must(t, tb.SetHeader("A", "B"))
		must(t, tb.AddRow("a very long cell that has to wrap across several lines", "x"))
		must(t, d.AddTable(tb))
	}))
}

func TestLongTableSplitsAcrossPagesAndRepeatsTheHeader(t *testing.T) {
	pdf := render(t, func(d *pdfgen.Document) {
		tb, err := pdfgen.NewTable([]float32{100, 100}, 10)
		must(t, err)
		defer tb.Close()
		must(t, tb.SetHeader("HEADERCELL", "Value"))
		for i := range 150 {
			must(t, tb.AddRow(fmt.Sprintf("row %d", i), fmt.Sprint(i)))
		}
		must(t, d.AddTable(tb))
	})
	path := checked(t, "table_pages.pdf", pdf)
	pages := pageCount(t, path)
	if pages < 2 {
		t.Fatalf("pages = %d, want at least 2", pages)
	}
	if got := strings.Count(qdf(t, path), "HEADERCELL"); got != pages {
		t.Errorf("header appears %d times over %d pages", got, pages)
	}
}

func TestTableIsCopiedSoClosingItAfterwardsIsSafe(t *testing.T) {
	d, err := pdfgen.NewDocument()
	must(t, err)
	defer d.Close()
	tb := sampleTable(t)
	must(t, d.AddTable(tb))
	tb.Close()
	tb.Close()
	pdf, err := d.Render()
	must(t, err)
	checked(t, "table_closed.pdf", pdf)
}

func TestBadTableArguments(t *testing.T) {
	_, err := pdfgen.NewTable(nil, 10)
	wantError(t, err, -3, "")
	_, err = pdfgen.NewTable([]float32{0}, 10)
	wantError(t, err, -3, "")
	_, err = pdfgen.NewTable([]float32{100}, -1)
	wantError(t, err, -3, "")

	tb, err := pdfgen.NewTable([]float32{50, 50}, 10)
	must(t, err)
	defer tb.Close()
	if err := tb.AddRow("a\x00b"); err == nil || !strings.Contains(err.Error(), "NUL") {
		t.Errorf("NUL in cell: err = %v", err)
	}
	must(t, tb.AddRow("a short row is fine"))
	// A row with more cells than columns is only detected when the document is laid out.
	must(t, tb.AddRow("a", "b", "c"))
	d, err := pdfgen.NewDocument()
	must(t, err)
	defer d.Close()
	must(t, d.AddTable(tb))
	_, err = d.Render()
	wantError(t, err, -4, "more cells than columns")
}
