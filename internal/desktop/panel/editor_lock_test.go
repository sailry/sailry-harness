package panel

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestDocumentEditingLock(t *testing.T) {
	for _, path := range []string{"note.txt", "note.md"} {
		t.Run(path, func(t *testing.T) {
			pr := editorFixture(path, "Original text")
			locked := false
			pr.DocumentsLocked = func() bool { return locked }
			item := pr.Files.item(path)
			var view *ui.Context
			tester := ui.NewTester(func(c *ui.Context) {
				view = c
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { pr.liveFileEditor(c, path, true) })
			}, 800, 600)
			tester.SetLocale("en")
			tester.Frame()
			tester.Frame()
			label := "Code editor"
			if path == "note.md" {
				label = "Rich text"
			}
			field, ok := tester.Access(label)
			if !ok {
				t.Fatal("editor is missing", label)
			}
			tester.ClickAt(field.Bounds.X+40, field.Bounds.Y+8)
			tester.Frame()
			if path == "note.md" {
				item.documentState().rich.SetSelection(0, 8)
				pr.runEditorAction(view, path, "link")
				pr.prompt.values[0] = "https://example.com"
			}
			locked = true
			// Installation waits until every window has built this read-only
			// frame. Native input before it is retained by the close check.
			tester.Frame()
			tester.Type("Unexpected input")
			tester.Frame()
			if item.editor != nil && !item.editor.ReadOnly {
				t.Fatal("the source editor remained writable during installation")
			}
			pr.FileChanged(path, "Late edit")
			pr.runEditorAction(view, path, "bold")
			pr.runEditorAction(view, path, "undo")
			if path == "note.md" {
				pr.submitPrompt()
				if !pr.prompt.open || pr.prompt.pending {
					t.Fatal("a blocked link edit discarded its form")
				}
				if item.documentState().rich.Text().Markdown() != "Original text" {
					t.Fatal("a retained rich editor action changed the locked document")
				}
			}
			if text, dirty := pr.FileDraft(path); text != "Original text" || dirty || len(pr.Unsaved()) != 0 {
				t.Fatalf("a locked document accepted edits: %q dirty=%v", text, dirty)
			}
			locked = false
			tester.Frame()
			if item.editor != nil && item.editor.ReadOnly {
				t.Fatal("a failed installation left the source editor read-only")
			}
			pr.FileChanged(path, "Editable again")
			if text, dirty := pr.FileDraft(path); text != "Editable again" || !dirty {
				t.Fatal("a failed installation did not restore editing")
			}
			pr.SetFileWritable(path, false)
			if pr.fileWritable(item) {
				t.Fatal("releasing the lock made a read-only file writable")
			}
		})
	}
	// A formatting action may already be queued for the source editor when
	// installation starts; it must not create a draft on the next frame.
	pr := editorFixture("note.md", "Original text")
	pr.editorSourceAction(nil, "note.md", "insert:rule", "")
	pr.DocumentsLocked = func() bool { return true }
	native := &editor.State{Buffer: editor.NewBuffer("Original text")}
	pr.Files.item("note.md").documentState().pending(native)
	if native.Buffer.String() != "Original text" || len(pr.Unsaved()) != 0 {
		t.Fatal("a deferred source edit changed the locked document")
	}
}

func TestDocumentCheckCapturesNativeEdits(t *testing.T) {
	for _, path := range []string{"note.txt", "note.md"} {
		t.Run(path, func(t *testing.T) {
			pr := editorFixture(path, "Original text")
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { pr.liveFileEditor(c, path, true) })
			}, 800, 600)
			tester.Frame()
			tester.Frame()
			item := pr.Files.item(path)
			if path == "note.md" {
				item.documentState().rich.SetText(richtext.ParseMarkdown("Native edit"))
			} else {
				item.editor.Insert("Native edit")
			}
			if item.dirty {
				t.Fatal("fixture synchronized its input before the close check")
			}
			if names := pr.Unsaved(); len(names) != 1 || names[0] != path {
				t.Fatal("close/install check missed native input before the next frame", names)
			}
			if draft, dirty := pr.FileDraft(path); draft == "Original text" || !dirty {
				t.Fatal("close/install check did not retain native input")
			}
		})
	}
}

func TestDocumentLockNativeInput(t *testing.T) {
	inputs := []struct {
		name    string
		prepare func(*ui.Tester)
		send    func(*ui.Tester)
	}{
		{name: "text", send: func(tester *ui.Tester) { tester.Type("Unexpected input") }},
		{name: "composition", prepare: func(tester *ui.Tester) { tester.Command("selectAll") }, send: func(tester *ui.Tester) { tester.Compose("Unexpected input", 16) }},
		{name: "cut", prepare: func(tester *ui.Tester) { tester.Command("selectAll") }, send: func(tester *ui.Tester) { tester.Command("cut") }},
		{name: "paste", prepare: func(tester *ui.Tester) { tester.SetClipboard("Unexpected input") }, send: func(tester *ui.Tester) { tester.Command("paste") }},
	}
	for _, path := range []string{"note.txt", "note.md"} {
		for _, input := range inputs {
			for _, locked := range []bool{false, true} {
				mode := "editable"
				if locked {
					mode = "locked"
				}
				t.Run(path+"/"+input.name+"/"+mode, func(t *testing.T) {
					pr := editorFixture(path, "Original text")
					lock := false
					pr.DocumentsLocked = func() bool { return lock }
					tester := ui.NewTester(func(c *ui.Context) {
						theme.Apply(c, "", theme.Light)
						ui.Column(c).Fill().Children(func() { pr.liveFileEditor(c, path, true) })
					}, 800, 600)
					tester.SetLocale("en")
					label := "Code editor"
					if path == "note.md" {
						label = "Rich text"
					}
					field, ok := tester.Access(label)
					if !ok {
						t.Fatal("editor is missing", label)
					}
					tester.ClickAt(field.Bounds.X+40, field.Bounds.Y+8)
					if input.prepare != nil {
						input.prepare(tester)
					}
					item := pr.Files.item(path)
					var version func() uint64
					if path == "note.txt" {
						version = item.editor.Buffer.Version
					} else {
						version = item.documentState().rich.Version
					}
					before := version()
					lock = locked
					if locked {
						tester.Frame()
					}
					input.send(tester)
					// Retain the original native object. A following frame may
					// replace or reset it after FileChanged rejects the input.
					// That must not hide an edit accepted before that frame.
					if changed := version() != before; changed == locked {
						t.Fatalf("native input changed=%v while locked=%v", changed, locked)
					}
					if locked {
						if draft, dirty := pr.FileDraft(path); draft != "Original text" || dirty {
							t.Fatal("locked native input changed the draft", draft, dirty)
						}
					}
				})
			}
		}
	}
}

func TestClosingEditorCopyIsReadOnly(t *testing.T) {
	for _, path := range []string{"note.txt", "note.md"} {
		t.Run(path, func(t *testing.T) {
			pr := editorFixture(path, "Original text")
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { pr.liveFileEditor(c, path, false) })
			}, 800, 600)
			tester.SetLocale("en")
			label := "Code editor"
			if path == "note.md" {
				label = "Rich text"
			}
			field, ok := tester.Access(label)
			if !ok {
				t.Fatal("editor is missing", label)
			}
			tester.ClickAt(field.Bounds.X+40, field.Bounds.Y+8)
			tester.Type("Unexpected input")
			if draft, dirty := pr.FileDraft(path); draft != "Original text" || dirty {
				t.Fatal("the closing editor copy accepted an unowned draft", draft, dirty)
			}
		})
	}
}

func TestDocumentPendingEditSurvivesLock(t *testing.T) {
	pr := editorFixture("note.md", "Original text")
	pr.editorSourceAction(nil, "note.md", "insert:rule", "")
	if !pr.PendingEdits() || len(pr.Unsaved()) != 0 {
		t.Fatal("the queued source edit is not distinguished from a dirty draft")
	}
	locked := true
	pr.DocumentsLocked = func() bool { return locked }
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { pr.liveFileEditor(c, "note.md", true) })
	}, 800, 600)
	tester.Frame()
	if !pr.PendingEdits() {
		t.Fatal("the locked frame discarded a queued source edit")
	}
	if text, dirty := pr.FileDraft("note.md"); text != "Original text" || dirty {
		t.Fatal("a locked frame applied the queued source edit", text, dirty)
	}
	locked = false
	tester.Frame()
	if pr.PendingEdits() {
		t.Fatal("the unlocked source editor did not apply its queued edit")
	}
	if text, dirty := pr.FileDraft("note.md"); text == "Original text" || !dirty {
		t.Fatal("the queued source edit did not become a retained draft", text, dirty)
	}
	pr.runEditorAction(nil, "note.md", "find")
	if pr.PendingEdits() {
		t.Fatal("opening Find was classified as a pending document mutation")
	}
}
