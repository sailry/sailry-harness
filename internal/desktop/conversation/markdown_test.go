package conversation_test

import (
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// gfmReply has a table after text, after display math, after a Mermaid
// block and inside a list item, then the rest of what a reply's Markdown
// reads beyond CommonMark.
const gfmReply = "Intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n" +
	"$$x^2$$\n\n| C | D |\n|---|---|\n| 3 | 4 |\n\n" +
	"```mermaid\nflowchart LR\nA --> B\n```\n\n| E | F |\n|---|---|\n| 5 | 6 |\n\n" +
	"- item\n\n  | G | H |\n  |---|---|\n  | 7 | 8 |\n\n" +
	"- [ ] open task\n- [x] done task\n\n" +
	"~~gone~~ kept\n\n" +
	"See https://example.com/page and <https://example.org>.\n\n" +
	"Note[^1].\n\n[^1]: The footnote.\n\n" +
	"> quoted\n\n" +
	"1. one\n   - nested\n     - deeper\n\n" +
	"```go\nfunc main() {}\n```\n\n" +
	"Inline $y^2$ here.\n\n" +
	"> [!NOTE]\n> An alert.\n"

// roles counts the elements of a role, and those among them checked.
func roles(tt *ui.Tester, role ui.Role) (n, checked int) {
	for _, a := range tt.AccessAll() {
		if a.Role == role {
			n++
			if a.Checked {
				checked++
			}
		}
	}
	return n, checked
}

// noRawTable fails when a table's source shows as text.
func noRawTable(t *testing.T, tt *ui.Tester, after string) {
	t.Helper()
	for _, text := range tt.Texts() {
		if strings.Contains(text, "|---|") || strings.Contains(text, "| ") {
			t.Fatalf("a table shows as its source after %q: %q", after, text)
		}
	}
}

// checkMarkdown is the reply's Markdown beyond CommonMark: binding math
// keeps the tables, task lists, strikethrough, autolinks, footnotes and
// alerts MyGo reads by default, in a finished reply and one streaming in.
func checkMarkdown(t *testing.T) {
	cells := []string{"A", "B", "1", "2", "C", "D", "3", "4", "E", "F", "5", "6", "G", "H", "7", "8"}
	whole := func(t *testing.T, tt *ui.Tester) {
		t.Helper()
		noRawTable(t, tt, "the whole reply")
		texts := tt.Texts()
		for _, cell := range cells {
			if !slices.Contains(texts, cell) {
				t.Errorf("cell %q is not a text of its own: %q", cell, texts)
			}
		}
		if n, _ := roles(tt, ui.RoleTable); n != 4 {
			t.Errorf("%d tables, want 4", n)
		}
		if n, checked := roles(tt, ui.RoleCheckBox); n != 2 || checked != 1 {
			t.Errorf("%d task boxes, %d checked, want 2 and 1", n, checked)
		}
		for _, want := range []string{"open task", "done task", "gone kept", "quoted", "nested", "deeper", "The footnote.", "An alert.",
			"See https://example.com/page and https://example.org.", "Note[1].", "Inline \uFFFC here."} {
			if !slices.Contains(texts, want) {
				t.Errorf("no text %q: %q", want, texts)
			}
		}
		for _, link := range []string{"https://example.com/page", "https://example.org", "[1]"} {
			if _, ok := tt.AccessByRole(ui.RoleLink, link); !ok {
				t.Errorf("%q is not a link", link)
			}
		}
		// The display formula and the flowchart are pictures; the Go block
		// is a code block under its language.
		if _, ok := tt.AccessByRole(ui.RoleImage, "flowchart"); !ok || pictures(tt) != 2 {
			t.Errorf("%d pictures, flowchart %v: want the formula and the flowchart", pictures(tt), ok)
		}
		if a, ok := tt.AccessByRole(ui.RoleTextField, "Code editor"); !ok || a.Value != "func main() {}" || !slices.Contains(texts, "go") {
			t.Errorf("the Go block: %v %q", ok, a.Value)
		}
	}
	t.Run("completed", func(t *testing.T) {
		mode := theme.Light
		// Taller than the reply, so all of it is built.
		turns := []conversation.Turn{{ID: "gfm", User: "Show it", Reply: conversation.Reply{Status: conversation.Completed,
			Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: gfmReply}}}}}
		s := conversation.New()
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", mode)
			ui.Column(c).Fill().Children(func() { conversation.View(c, s, "gfm", turns, nil, noActions{}) })
		}, 900, 1400)
		tt.SetLocale("en")
		tt.Frame()
		tt.Frame()
		whole(t, tt)
	})
	t.Run("streaming", func(t *testing.T) {
		// Written a character at a time: a table stands once its delimiter
		// row is whole, its rows do not show as their source after that
		// (nor before, at the top level), and nothing reports a failure.
		text := ""
		status := conversation.Running
		s := conversation.New()
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			turns := []conversation.Turn{{ID: "gfm", User: "Show it", Reply: conversation.Reply{Status: status,
				Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}}}
			ui.Column(c).Fill().Children(func() { conversation.View(c, s, "gfm", turns, nil, noActions{}) })
		}, 900, 1400)
		tt.SetLocale("en")
		delimiters := 0
		for i, r := range gfmReply {
			text = gfmReply[:i+len(string(r))]
			tt.Frame()
			// MyGo's stream holds a table's header back until its delimiter
			// row only at the top level: the header of the table in the
			// list item is a line of text until that row is whole.
			if inList := delimiters == 3 && strings.Contains(text, "| G"); !inList {
				noRawTable(t, tt, text)
			}
			if diagramFailed(tt) {
				t.Fatalf("an error shows after %q: %q", text, tt.Texts())
			}
			if strings.HasSuffix(text, "|---|---|\n") {
				delimiters++
				header := cells[(delimiters-1)*4:][:2]
				texts := tt.Texts()
				if n, _ := roles(tt, ui.RoleTable); n != delimiters || !slices.Contains(texts, header[0]) || !slices.Contains(texts, header[1]) {
					t.Fatalf("table %d does not stand once its delimiter row is whole: %d tables, %q", delimiters, n, texts)
				}
			}
		}
		if delimiters != 4 {
			t.Fatalf("%d delimiter rows written, want 4", delimiters)
		}
		status = conversation.Completed
		for range 4 {
			tt.Frame()
		}
		whole(t, tt)
	})
}
