package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

func TestSidebarMenuPinsAndRenames(t *testing.T) {
	a, tt := newTester(t)
	title := "Port the neutral theme tokens"
	if err := tt.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Pin session"); err != nil {
		t.Fatalf("%v: %q", err, tt.Menu())
	}
	tt.Frame()
	cv := a.Data.Find("sailry-go/theme")
	if !cv.Pinned || cv.PinOrder == nil || *cv.PinOrder != 2 {
		t.Fatalf("the pin was not appended: %v %v", cv.Pinned, cv.PinOrder)
	}
	// Rename in place: the menu opens an input with the title.
	if err := tt.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Rename"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Type("Theme tokens")
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if cv.Title != "Theme tokens" {
		t.Errorf("rename gave %q", cv.Title)
	}
}

func TestSidebarRenameKeepsEmptyTitleOut(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find("chat/regex")
	a.Sidebar.StartRename(cv)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Key(0, ui.KeyBackspace)
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if cv.Title != "Regex for semantic versions" || !tt.HasText(i18n.In("en", "session_title_required")) {
		t.Errorf("an empty title was taken: %q", cv.Title)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if !tt.HasText("Regex for semantic versions") {
		t.Error("Escape did not give the rename up")
	}
}

func TestSidebarReordersPins(t *testing.T) {
	a, _ := newTester(t)
	order := func() []string {
		var out []string
		for _, id := range a.Sidebar.Order()[:3] {
			out = append(out, id)
		}
		return out
	}
	before := order()
	a.Sidebar.MovePin(before[0], before[2], true)
	after := order()
	if after[2] != before[0] || after[0] != before[1] {
		t.Errorf("pins %v became %v", before, after)
	}
}

func TestProjectEditorAndRemoval(t *testing.T) {
	a, tt := newTester(t)
	clickButton(t, tt, "Add project")
	tt.Frame()
	click(t, tt, "Save preview")
	if !tt.HasText(i18n.In("en", "project_required")) {
		t.Fatalf("no validation toast: %q", tt.Texts())
	}
	n := len(a.Data.Projects)
	a.Sidebar.OpenEditor(nil)
	tt.Frame()
	clickField(t, tt, i18n.In("en", "project_path"))
	tt.Type("~/Work/new-tool")
	tt.Frame()
	click(t, tt, "Save preview")
	tt.Frame()
	if len(a.Data.Projects) != n+1 || a.Data.Projects[n].Name != "new-tool" {
		t.Fatalf("the project was not added with its suggested name: %d", len(a.Data.Projects))
	}
	a.Sidebar.SetCollapsed([]string{"sailry-go", "sailry"})
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if err := tt.RightClick("field-notes"); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Remove project"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	msg := i18n.In("en", "project_remove_named", "name", "~/Documents/field-notes")
	if !tt.HasText(msg) {
		t.Fatalf("the confirmation does not name the project: %q", tt.Texts())
	}
	click(t, tt, i18n.In("en", "project_remove_preview"))
	tt.Frame()
	if len(a.Data.Projects) != n {
		t.Error("the project was not removed")
	}
}

// clickField clicks the text field assistive technology names so.
func clickField(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	info, ok := tt.AccessByRole(ui.RoleTextField, name)
	if !ok {
		t.Fatalf("no field %q: %q", name, tt.Texts())
	}
	b := info.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
}

// Delete asks first, naming the session once; confirming removes it.
func TestSidebarDeleteConfirms(t *testing.T) {
	a, tt := newTester(t)
	title := "Port the neutral theme tokens"
	if err := tt.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem(i18n.In("en", "session_delete")); err != nil {
		t.Fatalf("%v: %q", err, tt.Menu())
	}
	tt.Frame()
	if a.Data.Find("sailry-go/theme") == nil {
		t.Fatal("Delete removed the session before the confirmation")
	}
	if !tt.HasText(i18n.In("en", "session_delete_named", "name", title)) {
		t.Fatalf("the confirmation does not name the session: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "session_delete"))
	tt.Frame()
	if a.Data.Find("sailry-go/theme") != nil {
		t.Fatal("the session was not deleted")
	}
	if tt.HasText(title) {
		t.Errorf("the deleted session still shows: %q", tt.Texts())
	}
}
