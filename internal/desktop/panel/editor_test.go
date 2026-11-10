package panel

import (
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestEditorContracts(t *testing.T) {
	t.Run("live page and closing panel share a file", func(t *testing.T) {
		for _, fixture := range []struct{ path, source string }{
			{"note.txt", "source text"}, {"note.md", "# Native rich"}, {"table.md", "| a | b |\n| - | - |\n| c | d |"}, {"note.ipynb", `{"nbformat":4,"metadata":{},"cells":[]}`},
		} {
			for _, ownedFirst := range []bool{true, false} {
				pr := editorFixture(fixture.path, fixture.source)
				item := pr.Files.item(fixture.path)
				if strings.HasSuffix(fixture.path, ".ipynb") {
					pr.SetEditorNotebook(fixture.path, &viewmodel.EditorNotebook{Path: fixture.path, Cells: []viewmodel.EditorNotebookCell{{Kind: "code", Source: "print(1)"}}})
				}
				var pending, owner *editor.State
				if fixture.path == "note.txt" {
					item.documentState().pending = func(native *editor.State) { pending = native }
				}
				tester := ui.NewTester(func(c *ui.Context) {
					theme.Apply(c, "", theme.Light)
					ui.Row(c).Fill().Children(func() {
						render := func(own bool) {
							ui.Column(c.Key(own)).Grow(1).MinWidth(0).FillHeight().Children(func() { pr.liveFileEditor(c, fixture.path, own) })
							if own {
								owner = item.editor
							}
						}
						render(ownedFirst)
						render(!ownedFirst)
					})
				}, 1000, 700)
				tester.SetLocale("en")
				tester.Frame()
				tester.Frame()
				if fixture.path == "note.txt" && (pending == nil || pending != owner || item.editor != owner) {
					t.Fatal("closing host stole the native source editor or pending action")
				}
			}
		}
	})
	t.Run("rich selection follows original source spans", func(t *testing.T) {
		for _, fixture := range []struct{ source, before, sourceBefore string }{
			{"# Head\n\nA &amp; B \\*界\\* **bold**", "Head\nA &", "# Head\n\nA &amp;"},
			{"# Head\n\nA &amp; B \\*界\\* **bold**", "Head\nA & B *界", "# Head\n\nA &amp; B \\*界"},
			{"before\n\n```\nλ🙂\n```\n\nafter", "before\nλ", "before\n\n```\nλ"},
			{"- one\n  - two\n- three", "one\ntw", "- one\n  - tw"},
			{"`a &amp; λ` tail", "a &amp; λ", "`a &amp; λ"},
		} {
			document := richtext.NewDocument(richtext.ParseMarkdown(fixture.source))
			at := utf8.RuneCountInString(fixture.before)
			document.SetSelection(at, at)
			mapped, selected, ok := richSourceSelection(fixture.source, document)
			if !ok || mapped != fixture.source || selected.Anchor != len(fixture.sourceBefore) || selected.Head != len(fixture.sourceBefore) {
				t.Fatalf("source caret lost: source=%q mapped=%q selection=%+v ok=%v", fixture.source, mapped, selected, ok)
			}
		}
		source := "## Heading\n\nstart **界🙂** and \\*x\\* end"
		document := richtext.NewDocument(richtext.ParseMarkdown(source))
		start := utf8.RuneCountInString("Heading\nstart ")
		document.SetSelection(start+2, start)
		mapped, selected, ok := richSourceSelection(source, document)
		if !ok || mapped != source || selected.Anchor != strings.Index(source, "🙂")+len("🙂") || selected.Head != strings.Index(source, "界") {
			t.Fatalf("backward Unicode selection lost: %q %+v %v", mapped, selected, ok)
		}

		source = "# Keep\n\nleft &fjlig; right"
		document = richtext.NewDocument(richtext.ParseMarkdown(source))
		at := utf8.RuneCountInString("Keep\nleft f")
		document.SetSelection(at, at)
		mapped, selected, ok = richSourceSelection(source, document)
		if !ok || mapped != "# Keep\n\nleft fj right" || selected.Head != len("# Keep\n\nleft f") || richtext.ParseMarkdown(mapped).String() != document.Text().String() {
			t.Fatalf("entity interior did not expand at its caret: %q %+v %v", mapped, selected, ok)
		}

		pr := editorFixture("note.md", "A &amp; B")
		item := pr.Files.item("note.md")
		state := item.documentState()
		state.rich = richtext.NewDocument(richtext.ParseMarkdown(item.draft))
		state.richSafe = true
		state.rich.SetSelection(3, 3)
		pr.editorSourceAction(nil, "note.md", "insert:rule", "")
		native := &editor.State{Buffer: editor.NewBuffer(item.draft)}
		if state.pending == nil {
			t.Fatal("source insertion did not retain its native caret")
		}
		state.pending(native)
		if got, _ := pr.FileDraft("note.md"); got != "A &amp;\n\n---\n\n B" {
			t.Fatalf("first source insertion used an old source caret: %q", got)
		}
	})
	t.Run("markdown preservation", func(t *testing.T) {
		for _, source := range []string{"# Heading\n\n**bold** and *italic*", "- [ ] task\n- [x] done", "```\ncode\n```"} {
			if !richMarkdownSafe(source) {
				t.Fatalf("supported document rejected: %q", source)
			}
		}
		for _, source := range []string{"| a | b |\n| - | - |\n| c | d |", "```go\npackage main\n```", "<div>content</div>", "word[^note]\n\n[^note]: definition", "3. third", "first\nsecond", "![image](a.png)", "[link](https://example.com \"title\")"} {
			if richMarkdownSafe(source) {
				t.Fatalf("lossy codec admitted: %q", source)
			}
		}
		original := "#   Title\n\nplain **bold**"
		pr := editorFixture("note.md", original)
		item := pr.Files.item("note.md")
		state := item.documentState()
		state.rich = richtext.NewDocument(richtext.ParseMarkdown(original))
		state.richOriginal, state.richCanonical = original, state.rich.Text().Markdown()
		state.richVersion = state.rich.Version()
		state.rich.InsertText("!")
		pr.syncRichDraft("note.md")
		if _, dirty := pr.FileDraft("note.md"); !dirty {
			t.Fatal("native edit lost dirty state")
		}
		state.rich.Undo()
		pr.syncRichDraft("note.md")
		if got, dirty := pr.FileDraft("note.md"); got != original || dirty {
			t.Fatalf("undo failed to restore exact source: %q dirty=%v", got, dirty)
		}
	})
	t.Run("source formatting keeps unrelated bytes", func(t *testing.T) {
		source := "<div>Keep</div>\n\nhello world\n\n```go\npackage main\n```"
		start := strings.Index(source, "world")
		got, selection := editMarkdownSource(source, editor.Selection{Anchor: start, Head: start + 5}, "bold", "")
		if got != strings.Replace(source, "world", "**world**", 1) || got[selection.Start():selection.End()] != "world" {
			t.Fatalf("format changed unrelated source: %q %+v", got, selection)
		}
		restored, _ := editMarkdownSource(got, selection, "bold", "")
		if restored != source {
			t.Fatal("toggle did not restore source")
		}
		body := "before\n- first\n- second\nafter"
		got, _ = editMarkdownSource(body, editor.Selection{Anchor: 7, Head: 24}, "block:heading2", "")
		if got != "before\n## first\n## second\nafter" {
			t.Fatalf("block selection leaked outside its lines: %q", got)
		}
		for _, kind := range []string{"table", "image", "rule", "code", "mermaid", "math", "chart", "html"} {
			if markdownInsertion(kind, "https://example.com/image.png") == "" {
				t.Fatalf("empty insertion: %s", kind)
			}
		}
	})
	t.Run("overflow preserves every control", func(t *testing.T) {
		groups := [][]editorControl{{{action: "a"}, {action: "b"}}, {{action: "c"}}}
		for _, test := range []struct {
			room            float32
			visible, hidden string
		}{{-1, "abc", ""}, {116, "abc", ""}, {115, "ab", "c"}, {64, "", "abc"}, {0, "", "abc"}} {
			visible, hidden := fitEditorControls(groups, test.room)
			flatten := func(groups [][]editorControl) string {
				var b strings.Builder
				for _, group := range groups {
					for _, item := range group {
						b.WriteString(item.action)
					}
				}
				return b.String()
			}
			if flatten(visible) != test.visible || flatten(hidden) != test.hidden {
				t.Fatalf("room %v split %q / %q", test.room, flatten(visible), flatten(hidden))
			}
		}
	})
	t.Run("close and unavailable preserve drafts", func(t *testing.T) {
		pr := editorFixture("note.md", "base\n")
		pr.FileChanged("note.md", "draft")
		pr.SetFileUnavailable("note.md", true)
		if draft, dirty := pr.FileDraft("note.md"); draft != "draft\n" || !dirty || pr.Files.item("note.md").writable {
			t.Fatal("failed refresh destroyed the draft or allowed writes")
		}
		closed := 0
		if !pr.requestEditorClose("note.md", func() { closed++; pr.Files.Close("note.md") }) || closed != 0 {
			t.Fatal("dirty tab closed without settlement")
		}
		request := pr.closing
		if !pr.requestEditorClose("note.md", func() { t.Fatal("replaced close continuation") }) || pr.closing != request {
			t.Fatal("close prompt was replaced")
		}
		pr.finishEditorClose(request)
		pr.finishEditorClose(request)
		if closed != 1 {
			t.Fatalf("close callback count %d", closed)
		}
		pr.SetEditorNotebook("note.md", &viewmodel.EditorNotebook{})
		if pr.EditorNotebook("note.md") != nil || pr.Files.items["note.md"] != nil {
			t.Fatal("late notebook response recreated a closed tab")
		}
	})
}

func editorFixture(path, source string) *Project {
	pr := NewLiveProject("worktree", &viewmodel.Workspace{Files: &viewmodel.FileNode{Children: []*viewmodel.FileNode{{Name: path, Path: path}}}}, &Actions{})
	pr.SetFile(path, source)
	pr.SetFileWritable(path, true)
	return pr
}
