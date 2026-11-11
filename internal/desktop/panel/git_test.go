package panel

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// pageTester shows a project's Files or Git page, in English.
func pageTester(t *testing.T, k Kind) (*Project, *ui.Tester) {
	t.Helper()
	pr := NewProjects(preview.Load().Workspace).Of("sailry-go")
	// Opaque surfaces: the frost of menus and popovers costs most of a
	// headless frame and none of the assertions.
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { Page(c, k, pr, func() {}) })
	}, 1100, 760, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	return pr, tt
}

// shot writes the last frame to $SAILRY_SNAPSHOTS/name.png, when set.
func shot(t *testing.T, tt *ui.Tester, name string) {
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	f, err := os.Create(filepath.Join(dir, name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tt.Image()); err != nil {
		t.Fatal(err)
	}
}

func en(key string) string { return i18n.In("en", key) }

// clickRole clicks the element of role named name.
func clickRole(t *testing.T, tt *ui.Tester, r ui.Role, name string, mods ui.Modifiers) {
	t.Helper()
	e, ok := tt.AccessByRole(r, name)
	if !ok {
		t.Fatalf("no %v %q: %q", r, name, tt.Texts())
	}
	b := e.Bounds
	tt.ClickAtWith(mods, b.X+b.W/2, b.Y+b.H/2)
	tt.Frame()
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
}

// rightClickRow right-clicks the tree row named name.
func rightClickRow(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	e, ok := tt.AccessByRole(ui.RoleTreeItem, name)
	if !ok {
		t.Fatalf("no row %q: %q", name, tt.Texts())
	}
	tt.RightClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
}

// clickMenu clicks the menu item named name.
func clickMenu(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	for _, r := range []ui.Role{ui.RoleMenuItem, ui.RoleMenuItemCheckBox} {
		if _, ok := tt.AccessByRole(r, name); ok {
			clickRole(t, tt, r, name, 0)
			return
		}
	}
	t.Fatalf("no menu item %q: %q", name, tt.Texts())
}

// The Git page's changes, menus, history and whole diff; each case on its own page.
func TestGitPage(t *testing.T) {
	// The changes start grouped tracked and untracked, as folders; the ⋯
	// menu's View options regroup them by staging and list them flat.
	t.Run("GroupedSections", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		for _, s := range []string{"Tracked · 3", "Untracked · 1", "docs", "example.md"} {
			if _, ok := tt.AccessByRole(ui.RoleTreeItem, s); !ok {
				t.Fatalf("no row %q: %q", s, tt.Texts())
			}
		}
		shot(t, tt, "git-grouped")
		clickRole(t, tt, ui.RoleMenuButton, en("git_actions"), 0)
		for _, s := range []string{en("git_view_options"), en("git_stash_menu"), en("git_tags"), en("git_output")} {
			if !tt.HasText(s) {
				t.Fatalf("the actions menu has no %q: %q", s, tt.Texts())
			}
		}
		clickMenu(t, tt, en("git_view_options"))
		if !tt.HasText(en("git_group_staged")) {
			t.Fatalf("View options does not open: %q", tt.Texts())
		}
		shot(t, tt, "git-actions-menu")
		clickMenu(t, tt, en("git_group_staged"))
		if pr.Git.nav.grouping != "staged" {
			t.Fatalf("grouping %q", pr.Git.nav.grouping)
		}
		for _, s := range []string{"Staged · 2", "Unstaged · 3"} {
			if _, ok := tt.AccessByRole(ui.RoleTreeItem, s); !ok {
				t.Fatalf("no group %q: %q", s, tt.Texts())
			}
		}
		// Collapsing a group hides its rows.
		clickRole(t, tt, ui.RoleTreeItem, "Staged · 2", 0)
		if _, ok := tt.AccessByRole(ui.RoleTreeItem, "legacy.go"); ok {
			t.Fatal("the closed group still lists its changes")
		}
		// A change opens its diff, selected in its group.
		clickRole(t, tt, ui.RoleTreeItem, "README.md", 0)
		if pr.Git.change != "README.md" || pr.Git.scope != "unstaged" {
			t.Fatalf("shown %q in %q", pr.Git.change, pr.Git.scope)
		}
		if row, ok := tt.AccessByRole(ui.RoleTreeItem, "README.md"); !ok || !row.Selected {
			t.Fatal("the change shown is not selected")
		}
	})
	// A change's right-click menu is the system's, acting on every change of
	// a folder; discarding asks first, naming them.
	t.Run("ChangeMenu", func(t *testing.T) {
		_, tt := pageTester(t, Changes)
		rightClickRow(t, tt, "docs")
		items := tt.Menu()
		if slices.Contains(items, en("git_open_source")) || !slices.Contains(items, en("git_discard_changes")) {
			t.Fatalf("a folder's menu %q", items)
		}
		if err := tt.ChooseMenuItem(en("git_discard_changes")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
		if !tt.HasText(en("git_discard_confirm") + "\ndocs/example.md") {
			t.Fatalf("no confirmation naming the change: %q", tt.Texts())
		}
	})
	// Commit's menu commits, amends or commits and pushes; Amend fills the
	// message with the last commit's and shows that it amends.
	t.Run("CommitSplitMenu", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		if b, ok := tt.AccessByRole(ui.RoleButton, en("git_commit")); !ok || !b.Disabled {
			t.Fatal("Commit is enabled without a message")
		}
		more, ok := tt.AccessByRole(ui.RoleMenuButton, en("more"))
		for _, a := range tt.AccessAll() {
			// The commit box's caret is the lower of the two.
			if a.Role == ui.RoleMenuButton && a.Name == en("more") && a.Bounds.Y > more.Bounds.Y {
				more = a
			}
		}
		if !ok {
			t.Fatalf("no commit menu: %q", tt.Texts())
		}
		tt.ClickAt(more.Bounds.X+more.Bounds.W/2, more.Bounds.Y+more.Bounds.H/2)
		tt.Frame()
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
		for _, s := range []string{en("git_commit_amend"), en("git_commit_push"), en("git_commit_sync")} {
			if !tt.HasText(s) {
				t.Fatalf("the commit menu has no %q: %q", s, tt.Texts())
			}
		}
		shot(t, tt, "git-commit-menu")
		clickMenu(t, tt, en("git_commit_amend"))
		if !pr.Git.amend || pr.Git.commit != pr.Data.Repo.Commits[0].Message || !tt.HasText(en("git_amend")) {
			t.Fatalf("amend %v, message %q", pr.Git.amend, pr.Git.commit)
		}
		// Committing would write the repository.
		clickRole(t, tt, ui.RoleButton, en("git_commit"), 0)
		if !tt.HasText(en("preview_notice")) {
			t.Fatalf("committing does not tell the preview: %q", tt.Texts())
		}
	})
	// The history lists the commits with their id, author and branch.
	t.Run("History", func(t *testing.T) {
		_, tt := pageTester(t, Changes)
		if err := tt.Click(en("git_view_history")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		for _, s := range []string{"Port the Git panel's grouped changes", "9f2c41ab", "Ada Chen", "split-view"} {
			if !tt.HasText(s) {
				t.Fatalf("the history has no %q: %q", s, tt.Texts())
			}
		}
		shot(t, tt, "git-history")
	})
	// Diff opens every change's diff in one item.
	t.Run("AllChanges", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		clickRole(t, tt, ui.RoleButton, en("git_diff"), 0)
		if pr.Git.change != allChanges || !tt.HasText(en("git_all_changes")) || !tt.HasText("legacy.go") {
			t.Fatalf("shown %q: %q", pr.Git.change, tt.Texts())
		}
		shot(t, tt, "git-all-changes")
	})
}

// The file tree's selection and keys; each case on its own page.
func TestFileTree(t *testing.T) {
	// The file tree selects as the files plugin does: a click one row, Cmd
	// another, Shift a range from the anchor, Shift+Down extends it, Cmd+A
	// selects every row; the right-click menu acts on the selection.
	t.Run("MultiSelect", func(t *testing.T) {
		pr, tt := pageTester(t, Files)
		v := &pr.Files
		selected := func() []string { return v.sel.paths(pr.visible()) }
		clickRole(t, tt, ui.RoleTreeItem, "go.mod", 0)
		if got := selected(); !slices.Equal(got, []string{"sailry-go/go.mod"}) || v.File() != "sailry-go/go.mod" {
			t.Fatalf("a click selected %q, shows %q", got, v.File())
		}
		clickRole(t, tt, ui.RoleTreeItem, "example.md", ui.Cmd)
		if got := selected(); !slices.Equal(got, []string{"sailry-go/docs/example.md", "sailry-go/go.mod"}) || v.File() != "sailry-go/go.mod" {
			t.Fatalf("Cmd-click selected %q, shows %q", got, v.File())
		}
		clickRole(t, tt, ui.RoleTreeItem, "kit", ui.Shift)
		want := []string{"sailry-go/docs/example.md", "sailry-go/internal", "sailry-go/internal/kit", "sailry-go/internal/shell", "sailry-go/internal/shell/shell.go"}
		if got := selected(); !slices.Equal(got, want) {
			t.Fatalf("Shift-click selected %q, want %q", got, want)
		}
		for _, p := range want {
			if row, ok := tt.AccessByRole(ui.RoleTreeItem, p[len(parentDir(p))+1:]); !ok || !row.Selected {
				t.Fatalf("row %q is not shown selected", p)
			}
		}
		shot(t, tt, "files-multiselect")
		tt.Key(ui.Shift, ui.KeyDown)
		tt.Frame()
		if got := selected(); !slices.Contains(got, "sailry-go/internal/kit/floating.go") || v.sel.current != "sailry-go/internal/kit/floating.go" {
			t.Fatalf("Shift+Down selected %q, cursor %q", got, v.sel.current)
		}
		// The menu acts on the selection, the rows inside a folder also
		// selected going with it.
		rightClickRow(t, tt, "kit")
		items := tt.Menu()
		if slices.Contains(items, en("files_rename")) || !slices.Contains(items, en("files_trash")) {
			t.Fatalf("the selection's menu %q", items)
		}
		if err := tt.ChooseMenuItem(en("files_copy_path")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if got := tt.Clipboard(); got != "sailry-go/docs/example.md\nsailry-go/internal" {
			t.Fatalf("copied %q", got)
		}
		// A right click outside the selection selects that row alone.
		rightClickRow(t, tt, "go.mod")
		if items := tt.Menu(); !slices.Contains(items, en("files_rename")) {
			t.Fatalf("a row's menu %q", items)
		}
		tt.CloseMenu()
		tt.Frame()
		if got := selected(); !slices.Equal(got, []string{"sailry-go/go.mod"}) {
			t.Fatalf("a right click selected %q", got)
		}
		clickRole(t, tt, ui.RoleTreeItem, "go.mod", 0)
		tt.Key(ui.Cmd, ui.KeyA)
		tt.Frame()
		if got := selected(); len(got) != len(pr.visible()) {
			t.Fatalf("Cmd+A selected %d of %d", len(got), len(pr.visible()))
		}
	})
	// Plain arrows move the selection, Left closes a folder and Enter opens
	// a file.
	t.Run("Keys", func(t *testing.T) {
		pr, tt := pageTester(t, Files)
		v := &pr.Files
		clickRole(t, tt, ui.RoleTreeItem, "docs", 0)
		if *v.folders["sailry-go/docs"] {
			t.Fatal("a click does not close a folder")
		}
		tt.Key(0, ui.KeyRight)
		tt.Frame()
		if !*v.folders["sailry-go/docs"] {
			t.Fatal("Right does not open a folder")
		}
		tt.Key(0, ui.KeyDown)
		tt.Frame()
		tt.Frame()
		if v.sel.current != "sailry-go/docs/example.md" || !v.sel.selected["sailry-go/docs/example.md"] || len(v.sel.selected) != 1 {
			t.Fatalf("Down moved the selection to %q: %v", v.sel.current, v.sel.selected)
		}
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if v.File() != "sailry-go/docs/example.md" {
			t.Fatalf("Enter showed %q", v.File())
		}
	})
}
