package pdfgen_test

// Helpers for the tests. Every PDF goes to go/test-output/ and must pass `qpdf --check`; the Arlington and
// content-stream checks then run over the directory via
// PDFGEN_CHECK_DIR=go/test-output cargo test -p pdfgen-core --test external_pdfs

import (
	"bytes"
	"encoding/binary"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	pdfgen "github.com/patcsy88/pdfgen/go"
)

var outDir string

func TestMain(m *testing.M) {
	wd, err := os.Getwd()
	if err != nil {
		panic(err)
	}
	outDir = filepath.Join(wd, "test-output")
	if err := os.RemoveAll(outDir); err != nil {
		panic(err)
	}
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		panic(err)
	}
	if _, err := exec.LookPath("qpdf"); err != nil {
		panic("qpdf must be installed to run these tests (apt install qpdf)")
	}
	os.Exit(m.Run())
}

func qpdf(t *testing.T, args ...string) (int, string) {
	t.Helper()
	cmd := exec.Command("qpdf", args...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		if ee, ok := err.(*exec.ExitError); ok {
			return ee.ExitCode(), string(out)
		}
		t.Fatalf("running qpdf: %v", err)
	}
	return 0, string(out)
}

// checked writes the PDF to test-output/, requires `qpdf --check` to exit 0, and returns the path.
func checked(t *testing.T, name string, pdf []byte) string {
	t.Helper()
	path := filepath.Join(outDir, name)
	if err := os.WriteFile(path, pdf, 0o644); err != nil {
		t.Fatal(err)
	}
	if status, out := qpdf(t, "--check", path); status != 0 {
		t.Fatalf("qpdf --check exited %d for %s:\n%s", status, name, out)
	}
	return path
}

func pageCount(t *testing.T, path string) int {
	t.Helper()
	status, out := qpdf(t, "--show-npages", path)
	if status != 0 {
		t.Fatalf("qpdf --show-npages: %s", out)
	}
	n, err := strconv.Atoi(strings.TrimSpace(out))
	if err != nil {
		t.Fatal(err)
	}
	return n
}

// qdf returns the PDF with all streams and object streams decompressed, as text.
func qdf(t *testing.T, path string) string {
	t.Helper()
	tmp := path + ".qdf"
	status, out := qpdf(t, "--qdf", "--object-streams=disable", path, tmp)
	if status != 0 && status != 3 {
		t.Fatalf("qpdf --qdf: %s", out)
	}
	data, err := os.ReadFile(tmp)
	if err != nil {
		t.Fatal(err)
	}
	os.Remove(tmp)
	return string(data)
}

func gradient(w, h, channels int) []byte {
	b := make([]byte, w*h*channels)
	for i := range b {
		b[i] = byte(i * 7)
	}
	return b
}

// grayJPEG is a minimal valid baseline grayscale JPEG (flat mid-gray); w and h are multiples of 8. One 8x8
// block is a DC code "0" plus an EOB code "0", each a 1-bit Huffman code. Port of gray_jpeg in
// core/tests/pdf.rs.
func grayJPEG(w, h int) []byte {
	bits := (w / 8) * (h / 8) * 2
	data := make([]byte, (bits+7)/8)
	if pad := len(data)*8 - bits; pad > 0 {
		data[len(data)-1] |= byte(1<<pad - 1)
	}
	var out bytes.Buffer
	out.Write([]byte{0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00})
	out.Write(bytes.Repeat([]byte{1}, 64))
	out.Write([]byte{0xff, 0xc0, 0x00, 0x0b, 0x08})
	binary.Write(&out, binary.BigEndian, uint16(h))
	binary.Write(&out, binary.BigEndian, uint16(w))
	out.Write([]byte{0x01, 0x01, 0x11, 0x00})
	for _, class := range []byte{0x00, 0x10} {
		out.Write([]byte{0xff, 0xc4, 0x00, 0x14, class, 0x01})
		out.Write(make([]byte, 15))
		out.WriteByte(0x00)
	}
	out.Write([]byte{0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00})
	out.Write(data)
	out.Write([]byte{0xff, 0xd9})
	return out.Bytes()
}

// render builds a document with build and returns its PDF bytes.
func render(t *testing.T, build func(d *pdfgen.Document)) []byte {
	t.Helper()
	d, err := pdfgen.NewDocument()
	if err != nil {
		t.Fatal(err)
	}
	defer d.Close()
	build(d)
	pdf, err := d.Render()
	if err != nil {
		t.Fatal(err)
	}
	return pdf
}

// must fails the test if err is not nil.
func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// wantError requires err to be a *pdfgen.Error with the given code and, if non-empty, a message containing msg.
func wantError(t *testing.T, err error, code int, msg string) {
	t.Helper()
	pe, ok := err.(*pdfgen.Error)
	if !ok {
		t.Fatalf("want *pdfgen.Error, got %T: %v", err, err)
	}
	if pe.Code != code {
		t.Errorf("code = %d, want %d (%s)", pe.Code, code, pe.Message)
	}
	if msg != "" && !strings.Contains(pe.Message, msg) {
		t.Errorf("message %q does not contain %q", pe.Message, msg)
	}
}

func contains(t *testing.T, text, want string) {
	t.Helper()
	if !strings.Contains(text, want) {
		t.Errorf("missing %q", want)
	}
}
