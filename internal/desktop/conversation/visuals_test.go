package conversation_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// replyTester shows one reply of Markdown, in a mode the test can switch.
func replyTester(text string, mode *theme.Mode) *ui.Tester {
	turns := []conversation.Turn{{User: "Draw it", Reply: conversation.Reply{Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}}}
	s := conversation.New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", *mode)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "visuals", turns, nil, noActions{}) })
	}, 900, 900)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	return tt
}

// pictures counts the pictures on screen.
func pictures(tt *ui.Tester) int {
	n := 0
	for _, a := range tt.AccessAll() {
		if a.Role == ui.RoleImage {
			n++
		}
	}
	return n
}

// checkMermaidBlocks is the headless smoke of ```mermaid blocks in a
// reply: limu draws the diagram, a theme switch and a narrower window keep
// it, and a source limu cannot read or a block with no end stays code.
func checkMermaidBlocks(t *testing.T) {
	t.Helper()
	mode := theme.Light
	tt := replyTester("Here:\n\n```mermaid\nflowchart LR\nA[\"quote ' </script>\"] --> B\n```\n\nDone.", &mode)
	d, ok := tt.AccessByRole(ui.RoleImage, "flowchart")
	if !ok || d.Bounds.W <= 0 || d.Bounds.H <= 0 {
		t.Fatalf("no diagram: %v %v", ok, d.Bounds)
	}
	if !tt.HasText("Done.") {
		t.Error("the text after the diagram is gone")
	}
	mode = theme.Dark
	tt.Frame()
	if dark, ok := tt.AccessByRole(ui.RoleImage, "flowchart"); !ok || dark.Bounds.W != d.Bounds.W || dark.Bounds.H != d.Bounds.H {
		t.Errorf("the theme switch changed the diagram: %v, was %v", dark.Bounds, d.Bounds)
	}
	tt.SetSize(420, 900)
	tt.Frame()
	tt.Frame()
	if narrow, ok := tt.AccessByRole(ui.RoleImage, "flowchart"); !ok || narrow.Bounds.W <= 0 {
		t.Errorf("no diagram in the narrow window: %v", narrow.Bounds)
	}

	for name, src := range map[string]string{
		"unreadable": "```mermaid\nthis is not a diagram\n```",
		"empty":      "```mermaid\n```",
		"open":       "```mermaid\nflowchart LR\nA --> B",
	} {
		mode = theme.Light
		if n := pictures(replyTester(src, &mode)); n != 0 {
			t.Errorf("%s source drew %d pictures, want its code", name, n)
		}
	}
}
