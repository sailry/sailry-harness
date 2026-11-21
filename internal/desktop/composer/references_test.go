package composer_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// Rows of the @ picker come from independent owners: the files, ssh and
// databases pages, the plugins, skills, sessions and agents. Their IDs are
// only unique within their own source, so a files row and a same-named
// plugin can share the bare ID "notes". A row's framework identity covers
// its Source, Kind and ID, so the two never share one state: with a shared
// key the Tester panics on the duplicate, and a running app would leak one
// row's pointer, selection and scrolling to the other.
func TestReferencePickerKeepsSameIDRowsDistinct(t *testing.T) {
	file := composer.Reference{ID: "notes", Source: "files", Kind: composer.File, Label: "notes", Icon: icons.File}
	plugin := composer.Reference{ID: "notes", Source: "plugins", Kind: composer.Plugin, Label: "Notes plugin", Icon: icons.Puzzle}

	// open draws the composer over the two colliding rows and opens the @
	// picker; reaching a drawn frame already proves the rows took no shared
	// key (the Tester panics on a duplicate).
	open := func(t *testing.T) (*ui.Tester, *composer.State) {
		t.Helper()
		state := composer.New(preview.DemoModel)
		tt := trayOf(state, &owner{}, "en", func() *composer.Context {
			return &composer.Context{References: []composer.Reference{file, plugin}, Activity: composer.Activity{Connected: true}}
		})
		if err := tt.Click(i18n.In("en", "composer")); err != nil {
			t.Fatal(err)
		}
		tt.Type("@")
		tt.Frame()
		return tt, state
	}

	t.Run("both rows show at once", func(t *testing.T) {
		tt, _ := open(t)
		if _, ok := tt.AccessByRole(ui.RoleOption, "notes"); !ok {
			t.Fatalf("the files row is missing: %q", tt.Texts())
		}
		if _, ok := tt.AccessByRole(ui.RoleOption, "Notes plugin"); !ok {
			t.Fatalf("the plugin row is missing: %q", tt.Texts())
		}
		options := 0
		for _, info := range tt.AccessAll() {
			if info.Role == ui.RoleOption {
				options++
			}
		}
		if options != 2 {
			t.Fatalf("want two distinct option rows, got %d: %q", options, tt.Texts())
		}
	})

	t.Run("each row is chosen on its own", func(t *testing.T) {
		tt, state := open(t)
		tt.Key(0, ui.KeyEnter) // the selection rests on the first row, the file
		tt.Frame()
		tt.Frame()
		if want := "@notes "; state.Draft != want {
			t.Fatalf("choosing the files row left the draft %q, want %q", state.Draft, want)
		}

		tt, state = open(t)
		tt.Key(0, ui.KeyDown) // move past the files row to the plugin
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		tt.Frame()
		if want := "@Notes plugin "; state.Draft != want {
			t.Fatalf("choosing the plugin row left the draft %q, want %q", state.Draft, want)
		}
	})
}
