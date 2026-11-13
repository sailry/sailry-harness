package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
		if r.saves != 1 || r.proceeded != 0 || !a.Closing() {
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

func TestNativeClosePrompt(t *testing.T) {
	for _, test := range []struct {
		name   string
		answer int
		saves  int
		closed int
	}{
		{"Cancel", 0, 0, 0},
		{"Discard", 1, 0, 1},
		{"Save", 2, 1, 0},
		{"Dismiss", -1, 0, 0},
	} {
		t.Run(test.name, func(t *testing.T) {
			a := New(preview.Load())
			r := &closeRequest{documents: []string{"notes.md", "main.go"}}
			var prompts []kit.Confirmation
			var answer func(int)
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) {
					prompts = append(prompts, request)
					answer = done
				})
				a.closeDialog(c)
			}, 600, 400)
			tester.SetLocale("en")
			a.ConfirmClose(r.request())
			tester.Frame()
			if len(prompts) != 1 || answer == nil {
				t.Fatal("close did not ask the native presenter once")
			}
			request := prompts[0]
			if !slices.Equal(request.Buttons, []string{i18n.In("en", "settings_cancel"), i18n.In("en", "files_unsaved_discard"), i18n.In("en", "files_save_all")}) || request.Default != 2 || request.Cancel != 0 || !slices.Equal(request.Destructive, []int{1}) {
				t.Fatalf("native close choices changed: %+v", request)
			}
			if tester.HasText(i18n.In("en", "files_unsaved")) {
				t.Fatal("native close also drew an application dialog")
			}
			answer(test.answer)
			tester.Frame()
			if r.saves != test.saves || r.proceeded != test.closed || a.Closing() != (test.answer == 2) {
				t.Fatalf("choice %d: saves=%d proceeded=%d closing=%v", test.answer, r.saves, r.proceeded, a.Closing())
			}
			if test.answer == 2 {
				tester.Advance(closePoll * 2)
				if len(prompts) != 1 || r.proceeded != 0 || !a.Closing() {
					t.Fatal("saving asked again or closed before the saves completed")
				}
				r.documents, r.saving = nil, false
				tester.Advance(closePoll * 2)
				if r.proceeded != 1 || a.Closing() {
					t.Fatal("completed saves did not close exactly once")
				}
			}
			answer(1)
			tester.Frame()
			wantClosed := test.closed
			if test.answer == 2 {
				wantClosed = 1
			}
			if r.proceeded != wantClosed || r.saves != test.saves {
				t.Fatal("a late native answer changed a completed close request")
			}
		})
	}
	t.Run("failed native save keeps the drafts and window", func(t *testing.T) {
		a := New(preview.Load())
		r := &closeRequest{documents: []string{"notes.md"}}
		var answer func(int)
		prompts := 0
		tester := ui.NewTester(func(c *ui.Context) {
			kit.BindConfirmations(c, func(_ kit.Confirmation, done func(int)) { prompts++; answer = done })
			a.closeDialog(c)
		}, 600, 400)
		a.ConfirmClose(r.request())
		tester.Frame()
		answer(2)
		tester.Frame()
		r.saving = false
		tester.Advance(closePoll * 2)
		if r.proceeded != 0 || r.saves != 1 || a.Closing() || len(r.documents) != 1 || prompts != 1 {
			t.Fatal("failed save discarded a draft, closed the window or prompted again")
		}
	})
	t.Run("quit over local work keeps Cancel and Quit mapping", func(t *testing.T) {
		for _, choice := range []int{0, 1} {
			a := New(preview.Load())
			r := &closeRequest{}
			var request kit.Confirmation
			var answer func(int)
			tester := ui.NewTester(func(c *ui.Context) {
				kit.BindConfirmations(c, func(value kit.Confirmation, done func(int)) { request, answer = value, done })
				a.closeDialog(c)
			}, 600, 400)
			tester.SetLocale("en")
			a.ConfirmClose(r.request())
			tester.Frame()
			if !slices.Equal(request.Buttons, []string{i18n.In("en", "settings_cancel"), i18n.In("en", "app_exit")}) || request.Cancel != 0 {
				t.Fatalf("native quit choices changed: %+v", request)
			}
			answer(choice)
			tester.Frame()
			if a.Closing() || r.proceeded != choice || r.saves != 0 {
				t.Fatalf("quit choice %d: proceeded=%d saves=%d", choice, r.proceeded, r.saves)
			}
		}
	})
}
