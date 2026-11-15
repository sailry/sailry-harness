package fileview

import (
	"image"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestDialog asserts the preview dialog draws a file as the panel does: its
// path over its text with Copy, a file it cannot draw by name, a file past
// the limit by its state, and Open only where the owner gives it.
func TestDialog(t *testing.T) {
	text := func(key string) string { return i18n.In("en", key) }
	draw := func(f conversation.File, cb Callbacks, progress ...float64) (*ui.Tester, *bool) {
		open, view := true, &View{}
		if len(progress) > 0 {
			view.Progress = progress[0]
		}
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
		// ⌘F opens the editor's find bar over the text.
		tester.Key(ui.Cmd, ui.KeyF)
		tester.Frame()
		if !tester.HasText("Find") {
			t.Fatalf("⌘F opened no find bar: %q", tester.Texts())
		}
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		if *open {
			t.Error("Escape left the preview open")
		}
	})
	t.Run("loading", func(t *testing.T) {
		tester, _ := draw(conversation.File{ID: "f", Path: "/srv/main.go", Kind: conversation.CodeFile, State: conversation.PreviewLoading}, Callbacks{}, 0.4)
		bar, ok := tester.AccessByRole(ui.RoleProgress, text("artifact_preview_loading"))
		if !ok || bar.Now < 0.39 || bar.Now > 0.41 {
			t.Fatalf("no progress bar while loading: %+v %q", bar, tester.Texts())
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

func TestImageDialog(t *testing.T) {
	file := conversation.File{ID: "picture", Path: "picture.png", Kind: conversation.ImageFile, State: conversation.PreviewLoading}
	open, view := true, &View{}
	var actions []kit.ImageAction
	cb := Callbacks{Image: func(action kit.ImageAction, shown conversation.Image) {
		if shown.ID != file.ID || shown.Source != file.Image.Source {
			t.Error("image action lost the picture's captured source")
		}
		actions = append(actions, action)
	}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		Dialog(c, "preview", view, file, &open, cb)
	}, 1200, 800)
	tester.SetLocale("en")
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	tester.Frame()
	assertViewer := func() {
		t.Helper()
		dialog, ok := tester.AccessByRole(ui.RoleDialog, i18n.In("en", "chat_image_preview"))
		if !ok || dialog.Bounds.W != 1200 || dialog.Bounds.H != 800 {
			t.Fatalf("picture did not open in the window's image viewer: %+v, %q", dialog, tester.Texts())
		}
	}
	assertViewer()
	tester.Key(0, ui.KeyEqual)
	tester.Frame()
	bitmap := ui.NewBitmap(image.NewNRGBA(image.Rect(0, 0, 400, 300)))
	file.State = conversation.PreviewReady
	file.Image = &conversation.Image{ID: file.ID, Name: file.Name(), Width: 400, Height: 300, Source: bitmap}
	tester.Frame()
	assertViewer()
	if !tester.HasText("125%") {
		t.Fatal("finishing the image read reset the viewer's zoom")
	}
	for _, key := range []string{"content_copy", "files_download", "files_open_system"} {
		if err := tester.Click(i18n.In("en", key)); err != nil {
			t.Fatal(err)
		}
	}
	if !slices.Equal(actions, []kit.ImageAction{kit.ImageCopy, kit.ImageSave, kit.ImageOpen}) {
		t.Fatalf("image toolbar actions = %v", actions)
	}
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	tester.Frame()
	if open {
		t.Fatal("Escape left the image viewer open")
	}
	if _, ok := tester.AccessByRole(ui.RoleDialog, i18n.In("en", "chat_image_preview")); ok {
		t.Fatal("a closed image viewer reopened on the next frame")
	}
}
