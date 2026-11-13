package panel

import (
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestNativeProjectConfirmation(t *testing.T) {
	pr := editorFixture("note.txt", "source")
	var requests []kit.Confirmation
	var answer func(int)
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) {
			requests = append(requests, request)
			answer = done
		})
		pr.promptDialog(c)
	}, 600, 400)
	tester.SetLocale("en")
	var completed func(bool)
	calls := 0
	pr.Confirm("files_trash", "note.txt", func(done func(bool)) { calls++; completed = done })
	tester.Frame()
	answer(1)
	tester.Frame()
	tester.Frame()
	if calls != 1 || len(requests) != 1 || !pr.prompt.pending || !pr.prompt.open {
		t.Fatal("native project confirmation lost or repeated its pending operation")
	}
	completed(false)
	tester.Frame()
	if len(requests) != 2 || calls != 1 || !pr.prompt.open {
		t.Fatal("failed project operation was retried without another confirmation")
	}
	answer(0)
	tester.Frame()
	if pr.prompt.open || calls != 1 {
		t.Fatal("cancelling the retry submitted another operation")
	}
	pr.Prompt("files_new_file", []string{"settings_name"}, []string{"draft.txt"}, nil)
	tester.Frame()
	if len(requests) != 2 {
		t.Fatal("an input form requested a native confirmation")
	}
	field, ok := tester.AccessByRole(ui.RoleTextField, en("settings_name"))
	if !ok || field.Disabled || field.Value != "draft.txt" {
		t.Fatalf("the input form lost its editable draft: field=%+v present=%v", field, ok)
	}
}

func TestNativeFileConfirmationChoices(t *testing.T) {
	t.Run("unwritable draft only offers Cancel and Discard", func(t *testing.T) {
		pr := editorFixture("note.txt", "source")
		pr.FileChanged("note.txt", "draft")
		pr.SetFileWritable("note.txt", false)
		closed := 0
		pr.requestEditorClose("note.txt", func() { closed++ })
		var request kit.Confirmation
		var answer func(int)
		tester := ui.NewTester(func(c *ui.Context) {
			kit.BindConfirmations(c, func(value kit.Confirmation, done func(int)) { request, answer = value, done })
			pr.editorCloseDialog(c)
		}, 600, 400)
		tester.SetLocale("en")
		tester.Frame()
		if !slices.Equal(request.Buttons, []string{en("settings_cancel"), en("files_discard")}) || request.Default != 1 || request.Cancel != 0 {
			t.Fatalf("an unavailable save was offered: %+v", request)
		}
		answer(0)
		tester.Frame()
		if _, dirty := pr.FileDraft("note.txt"); !dirty || closed != 0 || pr.closing != nil {
			t.Fatal("cancel discarded or closed an unwritable draft")
		}
	})
	t.Run("conflict without reload maps Overwrite to its displayed index", func(t *testing.T) {
		pr := editorFixture("note.txt", "source")
		pr.FileChanged("note.txt", "draft")
		writes := 0
		pr.FileConflict("note.txt", func(*ui.Context) { writes++ }, nil)
		var request kit.Confirmation
		var answer func(int)
		tester := ui.NewTester(func(c *ui.Context) {
			kit.BindConfirmations(c, func(value kit.Confirmation, done func(int)) { request, answer = value, done })
			pr.editorConflictDialog(c)
		}, 600, 400)
		tester.SetLocale("en")
		tester.Frame()
		if !slices.Equal(request.Buttons, []string{en("files_keep_editing"), en("files_overwrite")}) || request.Default != 1 || request.Cancel != 0 || !strings.Contains(request.Message, "note.txt") {
			t.Fatalf("conflict choices lost their target or callback: %+v", request)
		}
		answer(1)
		tester.Frame()
		if writes != 1 || pr.conflict != nil {
			t.Fatal("native overwrite did not perform its displayed choice once")
		}
	})
}
