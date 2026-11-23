package engine

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRepeatedFileReadAndCanonicalReadback(t *testing.T) {
	t.Parallel()
	const reads, lines, pageLimit = 8, 180, 8192
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	var file strings.Builder
	for i := range lines {
		fmt.Fprintf(&file, "const fixture%03d = %q\n", i, strings.Repeat("value", 14))
	}
	if err := os.WriteFile(filepath.Join(root, "fixture.go"), []byte(file.String()), 0o600); err != nil {
		t.Fatal(err)
	}
	read := objectTool("read_file", true, true, func(ctx context.Context, _ canonical.ToolRequest) ToolResult {
		result, err := files.Inspect(ctx, root, protocol.Command{Kind: "read_file_lines", Data: &protocol.CommandReadFileLinesPayload{Path: "fixture.go", Limit: lines}}, core.RestrictedFiles, nil)
		if err != nil {
			return toolFault(err)
		}
		return ToolResult{Value: result}
	})
	m := &scriptedModel{}
	var source protocol.ToolOutputOutputReference
	var pageArgs string
	for i := range reads {
		fileID := fmt.Sprintf("file-%d", i)
		pageID := fmt.Sprintf("page-%d", i)
		m.scripts = append(m.scripts,
			response(call(fileID, "read_file", `{}`), workingSetUsage(1500)),
			func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
				preview := pressureToolText(request, fileID)
				var receipt struct {
					Output protocol.ToolOutputOutputReference `json:"output"`
				}
				if err := json.Unmarshal([]byte(preview[strings.LastIndexByte(preview, '\n')+1:]), &receipt); err != nil || receipt.Output.Call != fileID || receipt.Output.Revision == "" {
					t.Errorf("file read %d lost its canonical reference", i)
					return
				}
				if i == 0 {
					source = receipt.Output
					pageArgs = fmt.Sprintf(`{"output":{"call":%q,"revision":%q},"offset":0,"limit":%d}`, source.Call, source.Revision, pageLimit)
				}
				if i == 1 && !strings.Contains(preview, "are unchanged since an earlier read_file result") {
					t.Error("immediately repeated read did not project the unchanged file")
				}
				response(call(pageID, "read_tool_output", pageArgs), workingSetUsage(1500))(ctx, request, yield)
			},
		)
	}
	m.scripts = append(m.scripts, response(text("Readback complete."), workingSetUsage(1500)))
	f := newFixtureWithTimeout(t, m, 20*time.Second, read)
	setWorkingSetWindow(t, f)
	f.start()
	turn := f.submit("Inspect the same file and retrieve the original tool output.")
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	stored := pressureStoredResults(f.state().Records)
	original := stored["file-0"]
	if original == nil || original.Status != canonical.ToolSuccess || original.Revision != source.Revision || len(original.Result.Value) <= pageLimit {
		t.Fatal("first file read was not retained at its original revision")
	}
	for i := range reads {
		file := stored[fmt.Sprintf("file-%d", i)]
		if file == nil || file.Status != canonical.ToolSuccess || file.Revision != original.Revision || string(file.Result.Value) != string(original.Result.Value) {
			t.Fatalf("canonical file read %d changed", i)
		}
		result := stored[fmt.Sprintf("page-%d", i)]
		var page protocol.ToolOutputOutputPage
		if result == nil || result.Status != canonical.ToolSuccess || json.Unmarshal(result.Result.Value, &page) != nil || page.Turn != turn || page.Output != source || page.Offset != 0 || page.NextOffset == nil || *page.NextOffset != pageLimit || page.TotalBytes != uint64(len(original.Result.Value)) || page.Text != string(original.Result.Value[:pageLimit]) {
			t.Fatalf("canonical readback %d did not return the original bytes", i)
		}
	}
}
