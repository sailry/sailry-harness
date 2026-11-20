package kit

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestDocumentHTMLMermaidFence(t *testing.T) {
	const source = "<pre><code class=\"language-mermaid\">flowchart TB\nA --&gt; B</code></pre>"
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		Markdown(c, source, MarkdownProps{})
	}, 800, 600)
	tester.Frame()
	if picture, ok := tester.AccessByRole(ui.RoleImage, "flowchart"); !ok || picture.Bounds.W <= 0 || picture.Bounds.H <= 0 {
		t.Fatal("the nested Markdown view of a document HTML block lost its Mermaid renderer")
	}
}

func TestMarkdownMermaidFenceDispatch(t *testing.T) {
	const source = "```mermaid\nflowchart TB\nA --> B\n```\n\n```mermaid\nunreadable diagram\n```\n\n```other\ncustom language\n```\n\n```chart\nordinary code\n```"
	handlers := map[string]func(*ui.Context, *markdown.CodeBlockView){
		"other": func(c *ui.Context, b *markdown.CodeBlockView) { ui.Text(c, "handler: "+strings.TrimSpace(b.Code())) },
	}
	options := markdown.Options{Code: handlers, CodeBlock: func(c *ui.Context, b *markdown.CodeBlockView) {
		ui.Text(c, "source: "+strings.TrimSpace(b.Code()))
	}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		Markdown(c, source, MarkdownProps{Options: &options})
	}, 800, 700)
	tester.Frame()
	if picture, ok := tester.AccessByRole(ui.RoleImage, "flowchart"); !ok || picture.Bounds.W <= 0 || picture.Bounds.H <= 0 {
		t.Fatal("the closed Mermaid fence is not a diagram")
	}
	for _, want := range []string{"source: unreadable diagram", "handler: custom language", "source: ordinary code"} {
		if !tester.HasText(want) {
			t.Errorf("missing %q in %q", want, tester.Texts())
		}
	}
	if handlers["mermaid"] != nil {
		t.Error("default renderer changed the caller's shared code map")
	}
	options.Code["mermaid"] = func(c *ui.Context, _ *markdown.CodeBlockView) { ui.Text(c, "custom diagram") }
	tester.Frame()
	if _, ok := tester.AccessByRole(ui.RoleImage, "flowchart"); ok || !tester.HasText("custom diagram") {
		t.Fatal("the caller's explicit Mermaid renderer did not take precedence")
	}
}
