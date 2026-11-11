package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// closeRequest is a close over documents with a save the test finishes.
type closeRequest struct {
	documents []string
	saving    bool
	saves     int
	proceeded int
}

func (r *closeRequest) request() CloseRequest {
	return CloseRequest{
		Documents: func() []string { return r.documents },
		Save:      func() { r.saves++; r.saving = true },
		Saving:    func() bool { return r.saving },
		Proceed:   func() { r.proceeded++ },
	}
}

func TestClosePrompt(t *testing.T) {
	en := func(key string) string { return i18n.In("en", key) }
	open := func(t *testing.T, documents ...string) (*App, *closeRequest, func(string), func() bool) {
		a, tt := newTester(t)
		r := &closeRequest{documents: documents}
		a.ConfirmClose(r.request())
		tt.Frame()
		tt.Advance(time.Second)
		press := func(key string) {
			t.Helper()
			clickButton(t, tt, en(key))
			tt.Frame()
			tt.Advance(time.Second)
		}
		shown := func() bool {
			tt.Frame()
			tt.Advance(time.Second)
			return tt.HasText(en("files_unsaved")) || tt.HasText(en("app_exit_confirm"))
		}
		t.Cleanup(func() {
			if t.Failed() {
				t.Logf("texts: %q", tt.Texts())
			}
		})
		return a, r, press, shown
	}
	t.Run("names the documents once and stays idle", func(t *testing.T) {
		a, tt := newTester(t)
		r := &closeRequest{documents: []string{"sailry/main.go", "sailry/notes.md"}}
		a.ConfirmClose(r.request())
		a.ConfirmClose(CloseRequest{Documents: func() []string { return []string{"other.go"} }})
		tt.Frame()
		tt.Advance(time.Second)
		for _, text := range []string{en("files_unsaved"), "sailry/main.go", "sailry/notes.md", en("files_unsaved_description"), en("files_save_all"), en("files_unsaved_discard")} {
			if !tt.HasText(text) {
				t.Fatalf("the prompt lacks %q: %q", text, tt.Texts())
			}
		}
		if tt.HasText("other.go") || tt.HasText(en("app_exit_confirm")) {
			t.Fatal("a second request replaced the prompt")
		}
	})
	t.Run("redraws only while a save is under way", func(t *testing.T) {
		// The preview shell animates on its own, so the prompt is built alone.
		a := New(preview.Load())
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			a.closeDialog(c)
		}, 800, 600)
		tt.SetLocale("en")
		r := &closeRequest{documents: []string{"a.go"}}
		a.ConfirmClose(r.request())
		tt.Frame()
		tt.Advance(time.Second)
		if tt.FramePending() {
			t.Fatal("the idle prompt keeps redrawing")
		}
		clickButton(t, tt, en("files_save_all"))
		tt.Advance(time.Second)
		if r.saves != 1 || !a.Closing() || r.proceeded != 0 {
			t.Fatalf("while saving: saves=%d closing=%v proceeded=%d", r.saves, a.Closing(), r.proceeded)
		}
		r.documents, r.saving = nil, false
		tt.Advance(time.Second)
		if a.Closing() || r.proceeded != 1 {
			t.Fatalf("the prompt did not notice the save's end by itself: closing=%v proceeded=%d", a.Closing(), r.proceeded)
		}
		tt.Advance(time.Second)
		if tt.FramePending() {
			t.Fatal("the closed prompt keeps redrawing")
		}
	})
	t.Run("Cancel keeps the window and the drafts", func(t *testing.T) {
		a, r, press, shown := open(t, "a.go")
		press("settings_cancel")
		if shown() || a.Closing() || r.proceeded != 0 || r.saves != 0 {
			t.Fatalf("after Cancel: closing=%v proceeded=%d saves=%d", a.Closing(), r.proceeded, r.saves)
		}
	})
	t.Run("Discard closes once without saving", func(t *testing.T) {
		a, r, press, shown := open(t, "a.go")
		press("files_unsaved_discard")
		if shown() || a.Closing() || r.proceeded != 1 || r.saves != 0 {
			t.Fatalf("after Discard: closing=%v proceeded=%d saves=%d", a.Closing(), r.proceeded, r.saves)
		}
	})
	t.Run("Save all closes after every save ends", func(t *testing.T) {
		a, r, press, shown := open(t, "a.go")
		press("files_save_all")
		if r.saves != 1 || r.proceeded != 0 || !shown() {
			t.Fatalf("while saving: saves=%d proceeded=%d", r.saves, r.proceeded)
		}
		r.documents, r.saving = nil, false
		if shown() || a.Closing() || r.proceeded != 1 {
			t.Fatalf("after the save: closing=%v proceeded=%d", a.Closing(), r.proceeded)
		}
	})
	t.Run("a failed save keeps the window open", func(t *testing.T) {
		a, r, press, shown := open(t, "a.go")
		press("files_save_all")
		r.saving = false
		if shown() || a.Closing() || r.proceeded != 0 {
			t.Fatalf("after the failure: closing=%v proceeded=%d", a.Closing(), r.proceeded)
		}
	})
	t.Run("a quit over local work confirms without documents", func(t *testing.T) {
		a, r, press, shown := open(t)
		if !shown() || a.Data == nil {
			t.Fatal("no quit confirmation")
		}
		press("app_exit")
		if shown() || r.proceeded != 1 {
			t.Fatalf("after Quit: proceeded=%d", r.proceeded)
		}
	})
}
