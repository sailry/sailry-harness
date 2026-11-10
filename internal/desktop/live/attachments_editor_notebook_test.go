package live

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"image"
	"image/png"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testEditorNotebookProjection(t *testing.T) {
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, image.NewNRGBA(image.Rect(0, 0, 2048, 16))); err != nil {
		t.Fatal(err)
	}
	imageValue := base64.StdEncoding.EncodeToString(encoded.Bytes())
	cell := map[string]any{"cell_type": "code", "source": []string{"print(", "1)"}, "outputs": []any{
		map[string]any{"output_type": "stream", "text": []string{"first\n", "second\n"}},
		map[string]any{"output_type": "error", "traceback": []string{"trace", "failure"}},
		map[string]any{"output_type": "error", "ename": "Error", "evalue": "message"},
		map[string]any{"output_type": "display_data", "data": map[string]any{"text/plain": "caption", "image/png": imageValue[:4] + "\n \t" + imageValue[4:]}},
		map[string]any{"output_type": "display_data", "data": map[string]any{"image/png": "invalid base64"}},
	}}
	raw, err := json.Marshal(cell)
	if err != nil {
		t.Fatal(err)
	}
	next := uint64(21)
	page, err := projectNotebookPage(protocol.NotebookPage{Path: "note.ipynb", Revision: "observed", Metadata: json.RawMessage(`{"language_info":{"name":"python"}}`), Cells: []protocol.NotebookCell{{Index: 20, Value: raw, Truncated: true}}, NextOffset: &next})
	if err != nil {
		t.Fatal(err)
	}
	if page.Path != "note.ipynb" || page.Revision != "observed" || page.Language != "python" || page.Next == nil || *page.Next != 21 || len(page.Cells) != 1 {
		t.Fatalf("notebook identity was lost: %+v", page)
	}
	got := page.Cells[0]
	if got.Index != 20 || got.Source != "print(1)" || !got.Truncated || strings.Join(got.Outputs, "|") != "first\nsecond\n|trace\nfailure|Error: message|caption" {
		t.Fatalf("notebook cell projection changed: %+v", got)
	}
	if len(got.Images) != 1 || got.Images[0].Width != 1024 || got.Images[0].Height != 8 {
		t.Fatalf("notebook image was not bounded: %+v", got.Images)
	}

	draft := `{"nbformat":4,"metadata":{"kernelspec":{"language":"javascript"}},"cells":[{"cell_type":"markdown","source":["# Draft\n","new text"]},{"cell_type":"raw","source":"raw"}]}`
	local, err := projectNotebookDraft("note.ipynb", "captured", draft)
	if err != nil || local.Revision != "captured" || local.Language != "javascript" || len(local.Cells) != 2 || local.Cells[0].Source != "# Draft\nnew text" || local.Next != nil {
		t.Fatalf("unsaved notebook did not project its own JSON: %+v, %v", local, err)
	}
	for _, invalid := range []string{`{`, strings.Replace(draft, `"nbformat":4`, `"nbformat":3`, 1), strings.Replace(draft, `"metadata":{"kernelspec":{"language":"javascript"}}`, `"metadata":null`, 1), strings.Replace(draft, `["# Draft\n","new text"]`, `[null]`, 1), strings.Replace(draft, `"cell_type":"raw"`, `"cell_type":"unknown"`, 1)} {
		if _, err := projectNotebookDraft("note.ipynb", "captured", invalid); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("invalid draft accepted: %q, %v", invalid, err)
		}
	}
}
