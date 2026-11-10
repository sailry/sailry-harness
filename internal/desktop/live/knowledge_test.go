package live

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testKnowledgeViewer covers the citation a link asks for, the answers the
// viewer accepts, the toast of each fault and paging by re-reading.
func testKnowledgeViewer(t *testing.T) {
	collection := protocol.KnowledgeID{7}
	chunk, revision := strings.Repeat("a", 64), strings.Repeat("b", 64)
	input, valid := knowledgeRequest(shell.Source{Collection: collection.String(), Chunk: chunk, Revision: revision})
	if !valid || input.Context || input.Offset != 0 || input.Limit != 16*1024 {
		t.Fatalf("request %+v %v", input, valid)
	}
	for _, source := range []shell.Source{
		{Collection: "docs", Chunk: chunk, Revision: revision},
		{Collection: collection.String(), Chunk: "frame", Revision: revision},
		{Collection: collection.String(), Chunk: chunk, Revision: "3"},
	} {
		if _, valid := knowledgeRequest(source); valid {
			t.Errorf("%+v passed", source)
		}
	}
	for code, key := range map[protocol.ErrorCode]string{
		protocol.ErrorCodeRevisionConflict: "knowledge_citation_changed", protocol.ErrorCodeNotFound: "knowledge_citation_changed",
		protocol.ErrorCodePermissionDenied: "knowledge_denied", protocol.ErrorCodeInvalidRequest: "knowledge_citation_invalid",
		protocol.ErrorCodeUnavailable: "knowledge_citation_unavailable",
	} {
		if got := knowledgeErrorKey(code); got != key {
			t.Errorf("%s shows %s", code, got)
		}
	}
	texts := []string{"first page ", "second page ", "third page"}
	answer := func(offset uint32) protocol.KnowledgeCitationPage {
		citation := protocol.KnowledgeCitation{Collection: collection, Chunk: chunk, Revision: revision, Path: "notes/frame.md",
			Position: protocol.KnowledgePosition{Kind: "lines", Data: &protocol.KnowledgePositionLinesPayload{Start: 12, End: 30}}}
		citation.Link = knowledge.CitationURL(citation)
		page := protocol.KnowledgeCitationPage{Citation: citation, Title: "frame.md", Offset: offset, Untrusted: true}
		at := uint32(0)
		for i, text := range texts {
			if at == offset {
				page.Text = text
				if i < len(texts)-1 {
					next := at + uint32(len(text))
					page.NextOffset = &next
				}
			}
			at += uint32(len(text))
		}
		return page
	}
	first := answer(0)
	if !knowledgeMatches(first, input) {
		t.Fatal("the first page was refused")
	}
	for name, change := range map[string]func(*protocol.KnowledgeCitationPage){
		"revision":  func(page *protocol.KnowledgeCitationPage) { page.Citation.Revision = strings.Repeat("c", 64) },
		"link":      func(page *protocol.KnowledgeCitationPage) { page.Citation.Link += "&path=x" },
		"offset":    func(page *protocol.KnowledgeCitationPage) { page.Offset = 4 },
		"context":   func(page *protocol.KnowledgeCitationPage) { page.Context = true },
		"untrusted": func(page *protocol.KnowledgeCitationPage) { page.Untrusted = false },
		"next":      func(page *protocol.KnowledgeCitationPage) { *page.NextOffset++ },
	} {
		page := answer(0)
		change(&page)
		if _, failure := knowledgePage(protocol.Output{Kind: "knowledge_citation", Data: page}, nil, input); failure != "knowledge_citation_unavailable" {
			t.Errorf("a page with another %s shows %q", name, failure)
		}
	}

	var requests []uint32
	var fault error
	var queue []func(*ui.Context)
	shown := &shell.Reading{}
	viewer := &knowledgeViewer{view: shown, shown: func() *shell.Reading { return shown }}
	viewer.read = func(input protocol.KnowledgeCitationRequest, done func(*ui.Context, protocol.Output, error)) {
		requests = append(requests, input.Offset)
		output, err := protocol.Output{Kind: "knowledge_citation", Data: answer(input.Offset)}, fault
		queue = append(queue, func(c *ui.Context) { done(c, output, err) })
	}
	opened := false
	tester := ui.NewTester(func(c *ui.Context) {
		if !opened {
			opened = true
			viewer.show(c, input, first)
		}
		for _, operation := range queue {
			operation(c)
		}
		queue = nil
	}, 640, 400)
	tester.SetLocale("en")
	tester.Frame()
	if shown.Title != "frame.md" || shown.Path != "notes/frame.md" || shown.Position != i18n.In("en", "knowledge_lines", "start", 12, "end", 30) ||
		shown.Text != texts[0] || !shown.First || shown.Last {
		t.Fatalf("first page %+v", shown)
	}
	viewer.turn(false)
	if len(requests) != 0 {
		t.Fatal("Previous read before the first page")
	}
	viewer.turn(true)
	viewer.turn(true)
	if len(requests) != 1 || !shown.Pending {
		t.Fatalf("a loading page read again: %v", requests)
	}
	tester.Frame()
	viewer.turn(true)
	tester.Frame()
	if shown.Text != texts[2] || shown.First || !shown.Last || shown.Pending {
		t.Fatalf("last page %+v", shown)
	}
	viewer.turn(true)
	if len(requests) != 2 {
		t.Fatal("Next read past the last page")
	}
	// A source that changed keeps the page and tells why.
	fault = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "knowledge source changed; refresh its results"}
	viewer.turn(false)
	tester.Frame()
	tester.Frame()
	if shown.Text != texts[2] || shown.Pending || !tester.HasText(i18n.In("en", "knowledge_citation_changed")) {
		t.Fatalf("stale page %+v, texts %q", shown, tester.Texts())
	}
	fault = nil
	viewer.turn(false)
	tester.Frame()
	viewer.turn(false)
	tester.Frame()
	if shown.Text != texts[0] || !shown.First || shown.Last || len(viewer.history) != 0 {
		t.Fatalf("back at the first page %+v", shown)
	}
	if want := []uint32{uint32(len(texts[0])), uint32(len(texts[0]) + len(texts[1])), uint32(len(texts[0])), uint32(len(texts[0])), 0}; len(requests) != len(want) {
		t.Fatalf("requests %v, want %v", requests, want)
	} else {
		for i := range want {
			if requests[i] != want[i] {
				t.Fatalf("requests %v, want %v", requests, want)
			}
		}
	}

	// The viewer is the last link's: an earlier link's late answer, here of
	// a slower Node, does not replace it.
	root := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	var answers []func(*ui.Context)
	later := input
	later.Chunk = strings.Repeat("d", 64)
	page := func(input protocol.KnowledgeCitationRequest, title string) protocol.KnowledgeCitationPage {
		citation := protocol.KnowledgeCitation{Collection: input.Collection, Chunk: input.Chunk, Revision: input.Revision, Path: title}
		citation.Link = knowledge.CitationURL(citation)
		return protocol.KnowledgeCitationPage{Citation: citation, Title: title, Untrusted: true}
	}
	for _, link := range []struct {
		node  string
		input protocol.KnowledgeCitationRequest
		title string
	}{{"remote", input, "earlier.md"}, {"", later, "later.md"}} {
		root.readKnowledge(link.node, link.input, func(input protocol.KnowledgeCitationRequest, done func(*ui.Context, protocol.Output, error)) {
			answers = append(answers, func(c *ui.Context) {
				done(c, protocol.Output{Kind: "knowledge_citation", Data: page(input, link.title)}, nil)
			})
		}, func(*ui.Context) string { return link.node })
	}
	order := ui.NewTester(func(c *ui.Context) {
		for _, index := range []int{1, 0} {
			answers[index](c)
		}
	}, 320, 200)
	order.Frame()
	if reading := root.App.Reading(); reading == nil || reading.Title != "later.md" || reading.Host != "" {
		t.Fatalf("after the earlier link answered last the viewer shows %+v", reading)
	}
}
