package output

import (
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProcessedSearchPage(t *testing.T) {
	page := protocol.DefaultSearchResults()
	page.Offset, page.Skipped, page.ScannedFiles, page.Partial = 1, 3, 217, true
	for i := range 100 {
		page.Matches = append(page.Matches, protocol.SearchMatch{Path: "pkg/example.go", LineNumber: uint64(i + 1), Line: fmt.Sprintf("func example%03d() { return }", i), Matched: true})
	}
	search := func() canonical.Record {
		return resultRecord("call", map[string]any{"kind": "search_results", "data": page}, canonical.JSON)
	}
	original := search()
	before := encoded(original)
	short, ok := Processed(original, original)
	if !ok || before != encoded(original) {
		t.Fatal("search page was not shortened independently of its source")
	}
	value := object(decode(onlyResponse(&short).Result.Value))
	if value["mode"] != "content" || value["partial"] != true || encoded(value["rows"]) != "100" || encoded(value["scanned_files"]) != "217" || encoded(value["skipped"]) != "3" || value["first_path"] != "pkg/example.go" || value["next_offset"] != nil {
		t.Fatalf("search summary lost scan facts: %s", encoded(value))
	}
	checkSkipped := func(record canonical.Record) {
		t.Helper()
		if _, ok := Processed(record, record); ok {
			t.Fatal("unsupported evidence was replaced")
		}
	}
	for _, mutate := range []func(*canonical.ToolResponse){
		func(tool *canonical.ToolResponse) { tool.Status = canonical.ToolErrorStatus },
		func(tool *canonical.ToolResponse) { tool.Format = canonical.MCP },
		func(tool *canonical.ToolResponse) { tool.Attachments = []canonical.Attachment{{}} },
		func(tool *canonical.ToolResponse) { tool.Result.Assets = []canonical.BinarySlot{{}} },
		func(tool *canonical.ToolResponse) { tool.Revision = strings.Repeat("x", 64) },
	} {
		r := search()
		mutate(onlyResponse(&r))
		checkSkipped(r)
	}
	page.Matches[0].Truncated = true
	checkSkipped(search())
	page.Matches[0].Truncated = false
	page.Diagnostics = []protocol.SearchDiagnostic{{Path: "secret", Message: "access denied"}}
	checkSkipped(search())
	checkSkipped(resultRecord("other", map[string]any{"kind": "other", "data": page}, canonical.JSON))
}
