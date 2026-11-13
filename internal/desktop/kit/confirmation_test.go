package kit

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type presentedConfirmation struct {
	request Confirmation
	answer  func(int)
}

type confirmationFixture struct {
	tester    *ui.Tester
	request   Confirmation
	open      bool
	mounted   bool
	presented []presentedConfirmation
	answers   []int
	actions   int
}

func newConfirmationFixture() *confirmationFixture {
	f := &confirmationFixture{
		open: true, mounted: true,
		request: Confirmation{Title: "Delete item", Message: "Delete Notes?", Buttons: []string{"Cancel", "Delete"}, Default: 0, Cancel: 0, Destructive: []int{1}},
	}
	f.tester = ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		BindConfirmations(c, func(request Confirmation, answer func(int)) {
			f.presented = append(f.presented, presentedConfirmation{request, answer})
		})
		if f.mounted {
			if answer := Confirm(c.Key("confirmation"), &f.open, f.request); answer >= 0 {
				f.answers = append(f.answers, answer)
				if answer == 1 {
					f.actions++
				}
			}
		}
	}, 600, 400)
	return f
}

func TestNativeConfirmation(t *testing.T) {
	t.Run("pending presents once and cancel never acts", func(t *testing.T) {
		f := newConfirmationFixture()
		for range 4 {
			f.tester.Frame()
		}
		if len(f.presented) != 1 || !f.open || f.actions != 0 {
			t.Fatalf("pending: prompts=%d open=%v actions=%d", len(f.presented), f.open, f.actions)
		}
		if f.tester.HasText(f.request.Title) || f.tester.HasText(f.request.Message) {
			t.Fatal("a native confirmation also drew an application dialog")
		}
		if _, ok := f.tester.AccessByRole(ui.RoleAlertDialog, f.request.Title); ok {
			t.Fatal("a native confirmation left an in-window alert")
		}
		f.presented[0].answer(0)
		f.tester.Frame()
		f.presented[0].answer(1)
		f.tester.Frame()
		if f.open || f.actions != 0 || !slices.Equal(f.answers, []int{0}) || len(f.presented) != 1 {
			t.Fatalf("cancel: prompts=%d open=%v actions=%d answers=%v", len(f.presented), f.open, f.actions, f.answers)
		}
	})
	t.Run("windows keep their own pending answer", func(t *testing.T) {
		first, second := newConfirmationFixture(), newConfirmationFixture()
		first.tester.Frame()
		second.tester.Frame()
		first.presented[0].answer(1)
		second.tester.Frame()
		if second.actions != 0 || !second.open || len(second.presented) != 1 {
			t.Fatal("another window received the first window's response")
		}
		first.tester.Frame()
		second.presented[0].answer(0)
		second.tester.Frame()
		if first.actions != 1 || first.open || second.actions != 0 || second.open {
			t.Fatalf("windows: first=%d/%v second=%d/%v", first.actions, first.open, second.actions, second.open)
		}
	})
	t.Run("dismissal and invalid responses cancel", func(t *testing.T) {
		for _, answer := range []int{-1, 2, 100} {
			f := newConfirmationFixture()
			f.tester.Frame()
			f.presented[0].answer(answer)
			f.tester.Frame()
			if f.open || f.actions != 0 || !slices.Equal(f.answers, []int{0}) {
				t.Fatalf("response %d: open=%v actions=%d answers=%v", answer, f.open, f.actions, f.answers)
			}
		}
	})
	t.Run("changed requests reject obsolete responses", func(t *testing.T) {
		for _, change := range []struct {
			name string
			run  func(*Confirmation)
		}{
			{"title", func(r *Confirmation) { r.Title = "Delete another item" }},
			{"message", func(r *Confirmation) { r.Message = "Delete another Notes?" }},
			{"buttons", func(r *Confirmation) { r.Buttons[1] = "Remove" }},
			{"default", func(r *Confirmation) { r.Default = 1 }},
			{"cancel", func(r *Confirmation) { r.Cancel = -1 }},
			{"destructive", func(r *Confirmation) { r.Destructive[0] = 0 }},
		} {
			t.Run(change.name, func(t *testing.T) {
				f := newConfirmationFixture()
				f.tester.Frame()
				change.run(&f.request)
				f.tester.Frame()
				if len(f.presented) != 2 {
					t.Fatalf("changed request was not presented: %d", len(f.presented))
				}
				f.presented[0].answer(1)
				f.tester.Frame()
				if f.actions != 0 || !f.open || len(f.answers) != 0 {
					t.Fatal("an obsolete response performed the new request")
				}
				f.presented[1].answer(1)
				f.tester.Frame()
				f.tester.Frame()
				if f.actions != 1 || f.open || !slices.Equal(f.answers, []int{1}) {
					t.Fatalf("current response: actions=%d open=%v answers=%v", f.actions, f.open, f.answers)
				}
			})
		}
	})
	t.Run("closing or unmounting invalidates a pending response", func(t *testing.T) {
		for _, unmount := range []bool{false, true} {
			f := newConfirmationFixture()
			f.tester.Frame()
			if unmount {
				f.mounted = false
			} else {
				f.open = false
			}
			f.tester.Frame()
			f.presented[0].answer(1)
			f.open, f.mounted = true, true
			f.tester.Frame()
			if f.actions != 0 || !f.open || len(f.presented) != 2 {
				t.Fatalf("obsolete response after unmount=%v: actions=%d open=%v prompts=%d", unmount, f.actions, f.open, len(f.presented))
			}
			f.presented[1].answer(1)
			f.tester.Frame()
			if f.actions != 1 || f.open {
				t.Fatalf("new request after unmount=%v did not complete once", unmount)
			}
		}
	})
}

func TestAlertDialogUsesNativeConfirmation(t *testing.T) {
	open := true
	var shown *presentedConfirmation
	var actions int
	tester := ui.NewTester(func(c *ui.Context) {
		BindConfirmations(c, func(request Confirmation, answer func(int)) {
			shown = &presentedConfirmation{request, answer}
		})
		if AlertDialog(c, &open, "Title", "Consequence", "Cancel", "Continue") == 1 {
			actions++
		}
	}, 600, 400)
	tester.Frame()
	if shown == nil || shown.request.Title != "Title" || shown.request.Message != "Consequence" || !slices.Equal(shown.request.Buttons, []string{"Cancel", "Continue"}) {
		t.Fatal("AlertDialog did not use the window's native presenter")
	}
	if shown.request.Default != 1 || shown.request.Cancel != 0 {
		t.Fatalf("confirmation default must be Continue and Escape must cancel: %+v", shown.request)
	}
	shown.answer(1)
	tester.Frame()
	if actions != 1 || open {
		t.Fatalf("AlertDialog result: actions=%d open=%v", actions, open)
	}
}
