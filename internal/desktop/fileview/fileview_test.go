package fileview

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestDialog asserts the preview dialog draws a file as the panel does: its
// path over its text with Copy, a file it cannot draw by name, a file past
// the limit by its state, and Open only where the owner gives it.
func TestDialog(t *testing.T) {
	text := func(key string) string { return i18n.In("en", key) }
	draw := func(f conversation.File, cb Callbacks) (*ui.Tester, *bool) {
		open, view := true, &View{}
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			if open {
				Dialog(c, "preview", view, f, &open, cb)
			}
		}, 1200, 800)
		tester.SetLocale("en")
		tester.SetPreferences(ui.Preferences{ReduceMotion: true})
		tester.Frame()
		tester.Frame()
		return tester, &open
	}
	t.Run("code", func(t *testing.T) {
		tester, open := draw(conversation.File{ID: "f", Path: "/srv/main.go", Kind: conversation.CodeFile, Text: "package main"}, Callbacks{})
		// The code shows in Kit's read-only code editor, which names itself.
		if !tester.HasText("/srv/main.go") || !tester.HasText("Code editor") {
			t.Fatalf("missing path or text: %q", tester.Texts())
		}
		if _, copy := tester.Access(text("content_copy")); !copy {
			t.Error("a text file has no Copy")
		}
		if _, external := tester.Access(text("files_open_system")); external {
			t.Error("Open shows without an owner to open the file")
		}
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		if *open {
			t.Error("Escape left the preview open")
		}
	})
	t.Run("unsupported", func(t *testing.T) {
		tester, _ := draw(conversation.File{ID: "f", Path: "/srv/app.bin", Kind: conversation.OtherFile}, Callbacks{OpenExternal: func() {}})
		if !tester.HasText("app.bin") || tester.HasText(text("content_copy")) {
			t.Fatalf("an unsupported file is not named alone: %q", tester.Texts())
		}
		if _, external := tester.Access(text("files_open_system")); !external {
			t.Fatal("no Open for a file its owner can open")
		}
	})
	t.Run("large", func(t *testing.T) {
		tester, _ := draw(conversation.File{ID: "f", Path: "/srv/huge.log", Kind: conversation.TextFile, State: conversation.PreviewLarge}, Callbacks{})
		if !tester.HasText(text("artifact_preview_large")) {
			t.Fatalf("no too-large state: %q", tester.Texts())
		}
		if _, copy := tester.Access(text("content_copy")); copy {
			t.Error("Copy shows for a file with no text read")
		}
	})
}
