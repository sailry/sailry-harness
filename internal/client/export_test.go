package client

import (
	"context"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationExport(t *testing.T) {
	t.Parallel()
	page := func(parts ...protocol.ConversationPart) protocol.ConversationPage {
		p := protocol.DefaultConversationPage()
		p.Session = protocol.NewSessionID()
		p.Revision = 1
		e := protocol.DefaultConversationEntry()
		e.ID = "user"
		e.Sequence = 1
		e.Turn = protocol.NewTurnID()
		e.Author = "user"
		e.Parts = parts
		p.Entries = []protocol.ConversationEntry{e}
		return p
	}
	text := func(s string) protocol.ConversationPart { return protocol.ConversationPart{Kind: "text", Data: s} }
	t.Run("final answer and closing interaction", func(t *testing.T) {
		p := page(text("User request"))
		turn := p.Entries[0].Turn
		for i, parts := range [][]protocol.ConversationPart{{text("Private progress"), {Kind: "thinking", Data: "Private reasoning"}, text("Final **answer**")}, {{Kind: "tool_call", Data: &protocol.ConversationPartToolCallPayload{Name: "plan_review"}}, {Kind: "resource", Data: map[string]any{"type": "tool_started"}}}} {
			e := protocol.DefaultConversationEntry()
			e.ID = string(rune('a' + i))
			e.Turn = turn
			e.Sequence = uint64(i + 2)
			e.Author = "assistant"
			e.Parts = parts
			p.Entries = append(p.Entries, e)
		}
		for _, format := range []ExportFormat{ExportMarkdown, ExportHTML} {
			d, err := composeExport(p, exportImages(p), "Conversation", format, "assets")
			if err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(d.Content, "User request") || !strings.Contains(d.Content, "Final ") || strings.Contains(d.Content, "Private") {
				t.Fatalf("final boundary was not respected: %s", d.Content)
			}
		}
	})
	t.Run("fragments and formulas", func(t *testing.T) {
		p := page(text("摘要 **完"), text("整**\n\n| Column | Value |\n| --- | --- |\n| Entry | 1 |\n\nInline $x^2$ and \\(y^2\\).\n\n\\[a < b\\]\n\n`\\(literal\\)`\n\n```mermaid\nflowchart LR\nA --> B\n```"))
		d, err := composeExport(p, nil, "Title", ExportHTML, "assets")
		if err != nil {
			t.Fatal(err)
		}
		for _, want := range []string{"<strong>完整</strong>", "<table>", `<span class="math inline">\(x^2\)</span>`, `\(y^2\)`, `\[a &lt; b\]`, `<code>\(literal\)</code>`, `class="language-mermaid"`} {
			if !strings.Contains(d.Content, want) {
				t.Errorf("missing %q in %s", want, d.Content)
			}
		}
	})
	t.Run("unsafe HTML and links", func(t *testing.T) {
		d, err := composeExport(page(text(`<script>alert(1)</script>`+"\n[link](javascript:alert) ![image](data:text/html,bad)")), nil, "<bad>", ExportHTML, "assets")
		if err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(d.Content, "&lt;script&gt;") || !strings.Contains(d.Content, "<title>&lt;bad&gt;</title>") || strings.Contains(d.Content, "<script>") || strings.Contains(d.Content, `href="javascript:`) || strings.Contains(d.Content, `src="data:`) {
			t.Fatal(d.Content)
		}
	})
	t.Run("attachment boundaries and conflicts", func(t *testing.T) {
		a := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: protocol.NewWorktreeID(), Name: "../中文 image.png", MediaType: "image/png", Size: 3, Revision: "fixture"}}
		p := page(text("Before"), protocol.ConversationPart{Kind: "attachment", Data: a}, text("After"))
		d, err := composeExport(p, nil, "Title", ExportMarkdown, "assets")
		if err != nil {
			t.Fatal(err)
		}
		if len(d.Attachments) != 1 || strings.Contains(ExportAttachmentName(a), "/") || !strings.Contains(d.Content, "Before\n\n[") || !strings.Contains(d.Content, ">)\n\nAfter") {
			t.Fatal(d)
		}
		a.Spec.Size++
		p.Entries[0].Parts = append(p.Entries[0].Parts, protocol.ConversationPart{Kind: "attachment", Data: a})
		if _, err = composeExport(p, nil, "Title", ExportMarkdown, "assets"); err == nil {
			t.Fatal("changed attachment identity was accepted")
		}
		if _, err = composeExport(page(), nil, "Title", ExportMarkdown, "../assets"); err == nil {
			t.Fatal("escaping asset path was accepted")
		}
	})
	t.Run("referenced images", func(t *testing.T) {
		p := page(text("Show result"))
		e := p.Entries[0]
		e.Sequence = 2
		e.ID = "answer"
		e.Author = "assistant"
		id := "call/é"
		image := protocol.ConversationImage{Entry: "result", Index: 2, Attachment: protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Name: "image.png"}}}
		e.Parts = []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &id, Name: "browser", Images: []protocol.ConversationImage{image}}}, text("![Result](sailry-image:call%2F%C3%A9/2) ![Missing](sailry-image:other/0)")}
		p.Entries = append(p.Entries, e)
		d, err := composeExport(p, exportImages(p), "Title", ExportMarkdown, "assets")
		if err != nil {
			t.Fatal(err)
		}
		if len(d.Images) != 1 || !strings.Contains(d.Content, "assets/"+ExportAttachmentName(image.Attachment)) || !strings.Contains(d.Content, "sailry-image:other/0") {
			t.Fatal(d)
		}
	})
	t.Run("notice user input", func(t *testing.T) {
		p := page(text("Automatic notice"))
		run := protocol.DefaultConversationRun()
		run.Turn = p.Entries[0].Turn
		run.Notice = true
		p.Runs = []protocol.ConversationRun{run}
		d, err := composeExport(p, nil, "Title", ExportMarkdown, "assets")
		if err != nil || d.Content != "" {
			t.Fatalf("automatic input was exported: %v %s", err, d.Content)
		}
	})
	t.Run("partial turn hydration", func(t *testing.T) {
		p := page(text("Original question"))
		turn := p.Entries[0].Turn
		run := protocol.DefaultConversationRun()
		run.Session = p.Session
		run.Worktree = protocol.NewWorktreeID()
		run.Turn = turn
		run.Revision = 1
		run.Sequence = 1
		run.Kind = protocol.ConversationRunKindTask
		run.Status = protocol.ConversationStatusCompleted
		p.Runs = []protocol.ConversationRun{run}
		user := p.Entries[0]
		reply := user
		reply.ID = "answer"
		reply.Author = "assistant"
		reply.Sequence = 3
		reply.Parts = []protocol.ConversationPart{text("Final answer")}
		p.Entries = []protocol.ConversationEntry{reply}
		calls := 0
		transport := &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
			calls++
			out := protocol.Output{}
			switch request.Command.Kind {
			case "read_conversation":
				out = protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Page: p, Missing: []protocol.TurnID{turn}}}
			case "read_turn":
				out = protocol.Output{Kind: "turn_history", Data: protocol.ConversationTurnHistory{Revision: 1, Run: run, Entries: []protocol.ConversationEntry{user}}}
			default:
				t.Errorf("unexpected export command %s", request.Command.Kind)
			}
			complete := make(chan link.Response, 1)
			complete <- link.Response{Output: out}
			return link.Admission{Completion: complete}, nil
		}}
		d, err := New(transport).ExportReply(context.Background(), p.Session, turn, "Title", ExportMarkdown, "assets")
		if err != nil {
			t.Fatal(err)
		}
		if calls != 2 || !strings.Contains(d.Content, "Original question") || !strings.Contains(d.Content, "Final answer") {
			t.Fatalf("incomplete export: %d %s", calls, d.Content)
		}
	})
	t.Run("cancelled before read", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, err := New(&clientTransport{}).ExportConversation(ctx, protocol.NewSessionID(), "Title", ExportMarkdown, "assets"); err == nil {
			t.Fatal("cancelled export succeeded")
		}
	})
}
