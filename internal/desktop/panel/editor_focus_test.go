package panel

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// A click in a writable file's editor, drawn by the panel beside its tree,
// gives it the keyboard focus and a caret, and what is typed reaches the
// file's draft. The tree's root has the path "": its row must not take the
// focus for a request nobody made.
func TestEditorTakesFocusBesideTree(t *testing.T) {
	const source = "import csv\n\n\ndef export_stock_csv(items, path):\n    pass\n"
	data := &viewmodel.Workspace{Files: &viewmodel.FileNode{Name: "project", Children: []*viewmodel.FileNode{{Name: "a.py", Path: "a.py"}}}}
	pr := NewLiveProject("worktree", data, &Actions{})
	pr.SetFile("a.py", source)
	pr.SetFileWritable("a.py", true)
	s := New(data)
	s.Show(Tab{Files, "a.py"})
	split, share := false, float32(0)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		kit.BindDiffSplit(c, &split)
		BindListShare(c, &share)
		ui.Column(c).Fill().Children(func() { View(c, s, pr, kit.KeyText) })
	}, 1000, 772, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	if _, ok := tt.Access("project"); !ok {
		t.Fatalf("the tree's root is not drawn: %q", tt.Texts())
	}
	editor, ok := tt.Access("Code editor")
	if !ok {
		t.Fatalf("no editor: %q", tt.Texts())
	}
	tt.ClickAt(editor.Bounds.X+120, editor.Bounds.Y+8)
	tt.Frame()
	tt.Frame()
	if _, caret := tt.TextCaret(); !tt.Focused("Code editor") || !caret {
		t.Fatalf("a click in the editor: focused %v, caret %v", tt.Focused("Code editor"), caret)
	}
	tt.Type("X")
	tt.Frame()
	tt.Frame()
	if draft, dirty := pr.FileDraft("a.py"); !dirty || draft == source {
		t.Fatalf("typing left the draft %q, dirty %v", draft, dirty)
	}
	if !pr.Files.item("a.py").documentState().wrap {
		t.Error("long lines do not wrap by default")
	}
}
