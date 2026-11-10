package node

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"image"
	"image/png"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// textPDF is a one-page document with a text layer.
func textPDF() []byte {
	content := "BT /F1 24 Tf 20 40 Td (Hello) Tj ET"
	objects := []string{
		"<< /Type /Catalog /Pages 2 0 R >>",
		"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
		"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
		fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", len(content), content),
		"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
	}
	var document bytes.Buffer
	document.WriteString("%PDF-1.4\n")
	offsets := make([]int, len(objects))
	for index, object := range objects {
		offsets[index] = document.Len()
		fmt.Fprintf(&document, "%d 0 obj\n%s\nendobj\n", index+1, object)
	}
	start := document.Len()
	fmt.Fprintf(&document, "xref\n0 %d\n0000000000 65535 f \n", len(objects)+1)
	for _, offset := range offsets {
		fmt.Fprintf(&document, "%010d 00000 n \n", offset)
	}
	fmt.Fprintf(&document, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(objects)+1, start)
	return document.Bytes()
}

// TestLocalModelRuntime checks document inspection on a Node that has no
// onnxruntime library beside its executable and no installed model: the
// runtimes are unavailable, OCR requests report their configuration fault and
// documents with text are read as before. Starting and stopping the Node
// takes most of its two seconds.
func TestLocalModelRuntime(t *testing.T) {
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		LocalModels: localmodels.Options{Capabilities: LocalModelCapabilities()},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	registry := node.localModels.registry

	var picture bytes.Buffer
	if err := png.Encode(&picture, image.NewGray(image.Rect(0, 0, 8, 8))); err != nil {
		t.Fatal(err)
	}
	code := func(err error) protocol.ErrorCode {
		var fault *protocol.Fault
		if errors.As(err, &fault) {
			return fault.Code
		}
		return ""
	}

	for _, kind := range []protocol.ModelsKind{protocol.ModelsKindEmbedding, protocol.ModelsKindOcr} {
		t.Run("unavailable "+string(kind), func(t *testing.T) {
			err := registry.Capabilities().Ensure(kind)
			if code(err) != protocol.ErrorCodeUnavailable || !strings.Contains(err.Error(), "ONNX Runtime file is unavailable") {
				t.Fatalf("Ensure = %v", err)
			}
		})
	}

	inspections := []struct {
		name    string
		file    string
		data    []byte
		code    protocol.ErrorCode
		message string
	}{
		{"image without a model", "scan.png", picture.Bytes(), protocol.ErrorCodeNotConfigured, "an OCR model must be installed"},
		{"image by extension only", "notes.JPEG", []byte("not an image"), protocol.ErrorCodeUnavailable, "OCR failed: "},
		{"oversized image", "huge.gif", []byte("GIF89a\x40\x1f\x40\x1f\x00\x00\x00"), protocol.ErrorCodeInvalidRequest, "OCR image dimensions exceed their limit"},
		{"unsupported file", "notes.txt", []byte("text"), protocol.ErrorCodeInvalidRequest, "supported extensions"},
	}
	for _, test := range inspections {
		t.Run(test.name, func(t *testing.T) {
			_, err := node.inspectDocument(t.Context(), test.file, test.data, 0)
			if code(err) != test.code || !strings.Contains(err.Error(), test.message) {
				t.Fatalf("inspectDocument error = %v, want %s %q", err, test.code, test.message)
			}
		})
	}

	t.Run("document with text", func(t *testing.T) {
		data := textPDF()
		want, err := office.Inspect("report.pdf", data, 0)
		if err != nil || len(want.Sections) != 1 || !strings.Contains(want.Sections[0].Text, "Hello") {
			t.Fatalf("office.Inspect = %#v, %v", want, err)
		}
		got, err := node.inspectDocument(t.Context(), "report.pdf", data, 0)
		if err != nil || !reflect.DeepEqual(got, want) {
			t.Fatalf("inspectDocument = %#v, %v", got, err)
		}
		if _, err := node.inspectDocument(t.Context(), "report.pdf", data, 2); code(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("offset error = %v", err)
		}
	})

	t.Run("OCR disabled", func(t *testing.T) {
		settings := registry.Settings()
		settings.Choices[protocol.ModelsKindOcr] = protocol.ModelsLocalChoice{}
		if err := registry.Configure(settings, nil); err != nil {
			t.Fatal(err)
		}
		_, err := node.inspectDocument(t.Context(), "scan.png", picture.Bytes(), 0)
		if code(err) != protocol.ErrorCodeNotConfigured || !strings.Contains(err.Error(), "OCR is disabled") {
			t.Fatalf("inspectDocument error = %v", err)
		}
	})
}
