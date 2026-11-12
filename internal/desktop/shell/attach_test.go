package shell

import (
	"fmt"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// attachmentPicker: Add attachment opens the window's one file picker in its
// multi-file mode, in the preview and in Demo mode, on the sample tree that
// holds the conversation's project; the chosen files become the draft's
// cards, in the composer that asked.
func attachmentPicker(t *testing.T) {
	en := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	rest := func(tt *ui.Tester) {
		for range 3 {
			settle(tt)
		}
	}
	// attach presses the n-th enabled Add attachment button, left to right.
	attach := func(t *testing.T, tt *ui.Tester, n int) {
		t.Helper()
		var found []ui.AccessInfo
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == en("composer_attachment") && !e.Disabled && e.Bounds.W > 0 {
				found = append(found, e)
			}
		}
		slices.SortFunc(found, func(x, y ui.AccessInfo) int { return int(x.Bounds.X - y.Bounds.X) })
		if n >= len(found) {
			t.Fatalf("%d attach buttons, want one at %d: %q", len(found), n, tt.Texts())
		}
		b := found[n].Bounds
		tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
		rest(tt)
	}
	// entry clicks the picker's row of that name, with mods.
	entry := func(t *testing.T, tt *ui.Tester, mods ui.Modifiers, name string) {
		t.Helper()
		all := tt.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == ui.RoleRow && all[i].Name == name {
				b := all[i].Bounds
				tt.ClickAtWith(mods, b.X+48, b.Y+b.H/2)
				settle(tt)
				return
			}
		}
		t.Fatalf("the picker has no entry %q: %q", name, tt.Texts())
	}
	// press clicks the topmost button of a name: the picker's.
	press := func(t *testing.T, tt *ui.Tester, name string) {
		t.Helper()
		all := tt.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == ui.RoleButton && all[i].Name == name && !all[i].Disabled {
				b := all[i].Bounds
				tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
				rest(tt)
				return
			}
		}
		t.Fatalf("no enabled button %q: %q", name, tt.Texts())
	}
	// names are the paths of the draft's files from the n-th on, sorted.
	names := func(comp *composer.State, n int) (out []string) {
		for _, a := range comp.Attachments[n:] {
			out = append(out, a.Path)
		}
		slices.Sort(out)
		return out
	}

	t.Run("preview", func(t *testing.T) {
		a, tt := newTester(t)
		comp := a.Composer(preview.Big)
		attach(t, tt, 0)
		if !a.Sidebar.Picker.Showing() {
			t.Fatalf("Add attachment did not open the window's picker: %q", tt.Texts())
		}
		// The sample project's own tree, under the sample account's home.
		for _, want := range []string{en("composer_attachment"), en("directory_preview"), en("directory_locations"), en("projects"),
			"docs", "internal", "README.md", "go.mod", en("directory_nothing_selected")} {
			if !slices.Contains(tt.Texts(), want) {
				t.Fatalf("the picker does not show %q: %q", want, tt.Texts())
			}
		}
		// Cancel attaches nothing.
		press(t, tt, en("settings_cancel"))
		if a.Sidebar.Picker.Showing() || len(comp.Attachments) != 0 {
			t.Fatalf("Cancel left the picker %v and %d attachments", a.Sidebar.Picker.Showing(), len(comp.Attachments))
		}
		// Two files, the second added with Cmd.
		attach(t, tt, 0)
		entry(t, tt, 0, "README.md")
		entry(t, tt, ui.Cmd, "go.mod")
		if !slices.Contains(tt.Texts(), en("directory_selected_files", "count", 2)) {
			t.Fatalf("Cmd did not add to the choice: %q", tt.Texts())
		}
		press(t, tt, en("directory_open"))
		if got, want := names(comp, 0), []string{"/Users/alex/Web/sailry-go/README.md", "/Users/alex/Web/sailry-go/go.mod"}; !slices.Equal(got, want) || a.Sidebar.Picker.Showing() {
			t.Fatalf("Open attached %q, want %q", got, want)
		}
		for _, card := range comp.Attachments {
			if card.Size <= 0 || card.Name == "" || !tt.HasText(card.Name) {
				t.Fatalf("the card %+v does not show the sample file", card)
			}
		}
		// The limit and its toast are the composer's, as for dropped files.
		for i := range composer.MaxAttachments - 2 {
			comp.Attachments = append(comp.Attachments, &composer.Attachment{ID: fmt.Sprintf("filler-%d", i), Name: fmt.Sprintf("filler-%d.txt", i), Status: composer.Ready})
		}
		attach(t, tt, 0)
		entry(t, tt, 0, "README.md")
		press(t, tt, en("directory_open"))
		if len(comp.Attachments) != composer.MaxAttachments || !tt.HasText(en("chat_attachment_limit")) {
			t.Fatalf("a ninth file: %d attachments, toast %v", len(comp.Attachments), tt.HasText(en("chat_attachment_limit")))
		}
	})

	t.Run("a split pane delivers to its own composer", func(t *testing.T) {
		a, tt := newTester(t)
		if !a.panes.Drop(session(preview.Big), session(themeChat), panes.Right) {
			t.Fatal("drop")
		}
		rest(tt)
		attach(t, tt, 1) // the right pane's
		if !a.Sidebar.Picker.Showing() {
			t.Fatalf("the pane's Add attachment did not open the picker: %q", tt.Texts())
		}
		entry(t, tt, 0, "go.mod")
		press(t, tt, en("directory_open"))
		if got := names(a.Composer(themeChat), 0); !slices.Equal(got, []string{"/Users/alex/Web/sailry-go/go.mod"}) || len(a.Composer(preview.Big).Attachments) != 0 {
			t.Fatalf("the right pane attached %q and the left %d files", got, len(a.Composer(preview.Big).Attachments))
		}
	})

	t.Run("demo", func(t *testing.T) {
		for _, mode := range []string{"dark", "light"} {
			a := New(preview.Load())
			a.Settings.Model.DemoMode = true
			tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
			tt.SetLocale("en")
			tt.SetDark(mode == "dark")
			tt.Frame()
			rest(tt)
			if !a.Demo() || a.Sidebar.Selected != preview.DemoStart {
				t.Fatalf("the demo is not on the window: %q", a.Sidebar.Selected)
			}
			comp := a.Composer(preview.DemoStart)
			before := len(comp.Attachments)
			attach(t, tt, 0)
			if !a.Sidebar.Picker.Showing() {
				t.Fatalf("the demo's Add attachment did not open the picker: %q", tt.Texts())
			}
			// The demo's project is browsable, and nothing names a sample.
			for _, want := range []string{"cmd", "docs", "internal", "go.mod", "README.md", "harbor-api", "lumen-web"} {
				if !slices.Contains(tt.Texts(), want) {
					t.Fatalf("the demo's picker does not show %q: %q", want, tt.Texts())
				}
			}
			entry(t, tt, 0, "README.md")
			entry(t, tt, ui.Cmd, "go.mod")
			tt.Move(4, 4)
			settle(tt)
			snapshot(t, tt, "attach-picker-demo-"+mode)
			press(t, tt, en("directory_open"))
			if got, want := names(comp, before), []string{"/Users/alex/Web/harbor-api/README.md", "/Users/alex/Web/harbor-api/go.mod"}; !slices.Equal(got, want) {
				t.Fatalf("the demo attached %q, want %q", got, want)
			}
			for _, sample := range []string{en("directory_preview"), en("preview_notice")} {
				if tt.HasText(sample) {
					t.Fatalf("the demo says %q: %q", sample, tt.Texts())
				}
			}
			snapshot(t, tt, "attach-picker-demo-chips-"+mode)
			// A folder opens with Enter; a double click attaches one file.
			attach(t, tt, 0)
			entry(t, tt, 0, "docs")
			tt.Key(0, ui.KeyEnter)
			rest(tt)
			if !slices.Contains(tt.Texts(), "booking.md") {
				t.Fatalf("Enter did not open the folder: %q", tt.Texts())
			}
			entry(t, tt, 0, "booking.md")
			tt.Key(0, ui.KeyEnter)
			rest(tt)
			if a.Sidebar.Picker.Showing() || !slices.Contains(names(comp, 0), "/Users/alex/Web/harbor-api/docs/booking.md") {
				t.Fatalf("Enter on a file did not attach it: %q", names(comp, 0))
			}
			// A subagent's session has no composer: nothing opens the picker.
			a.Sidebar.Selected = "lumen-web/audit"
			rest(tt)
			for _, e := range tt.AccessAll() {
				if e.Role == ui.RoleButton && e.Name == en("composer_attachment") && !e.Disabled {
					t.Fatal("a read-only conversation offers Add attachment")
				}
			}
		}
	})
}
