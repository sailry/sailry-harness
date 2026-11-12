package panel

import (
	"crypto/sha1"
	"encoding/hex"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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

func en(key string, args ...any) string { return i18n.In("en", key, args...) }

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
	// HEAD cannot restore a new file: discarding one discards as Discard all
	// does and says the file moves to Trash; a change HEAD holds is restored.
	t.Run("DiscardNewFile", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		var sent GitAction
		pr.Actions = &Actions{Git: func(_ *ui.Context, a GitAction) { sent = a }}
		pr.SetGitState(GitState{Known: true, Kind: "repository", Head: true})
		tt.Frame()
		for _, o := range []struct{ row, path, kind, message string }{
			{"floating.go", "internal/kit/floating.go", "git_discard_all", "git_discard_all_confirm"},
			{"README.md", "README.md", "git_discard_changes", "git_discard_confirm"},
		} {
			rightClickRow(t, tt, o.row)
			if err := tt.ChooseMenuItem(en("git_discard_changes")); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			tt.Advance(300 * time.Millisecond)
			tt.Frame()
			if cf := pr.Git.confirm; !cf.open || cf.message != en(o.message) || cf.target != o.path {
				t.Fatalf("%s: confirmation %+v", o.row, cf)
			}
			clickRole(t, tt, ui.RoleButton, en("git_discard_changes"), 0)
			if sent.Kind != o.kind || !slices.Equal(sent.Paths, []string{o.path}) {
				t.Fatalf("%s: sent %+v", o.row, sent)
			}
		}
		// Undo in the whole diff discards as Discard all does, for any change.
		clickRole(t, tt, ui.RoleButton, en("git_diff"), 0)
		// Undo sits inside the heading button, which is one accessible element.
		undo, ok := tt.Find(en("turn_changes_undo"))
		if !ok {
			t.Fatalf("no Undo: %q", tt.Texts())
		}
		tt.ClickAt(undo.X+undo.W/2, undo.Y+undo.H/2)
		tt.Frame()
		if cf := pr.Git.confirm; !cf.open || cf.message != en("git_discard_all_confirm") {
			t.Fatalf("Undo's confirmation %+v", cf)
		}
		clickRole(t, tt, ui.RoleButton, en("turn_changes_undo"), 0)
		if sent.Kind != "git_discard_all" || len(sent.Paths) != 1 {
			t.Fatalf("Undo sent %+v", sent)
		}
	})
	// Staging a conflicted file from its own row records its resolution;
	// Stage all, a folder and a group leave the conflict.
	t.Run("StageResolvesConflict", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		var sent GitAction
		pr.Actions = &Actions{Git: func(_ *ui.Context, a GitAction) { sent = a }}
		pr.SetGitState(GitState{Known: true, Kind: "repository", Head: true, Conflicts: map[string]bool{"README.md": true}})
		tt.Frame()
		if paths := pr.stagePaths(pr.Data.Changes, false); slices.Contains(paths, "README.md") || len(paths) == 0 {
			t.Fatalf("Stage all sends %q", paths)
		}
		rightClickRow(t, tt, "README.md")
		if err := tt.ChooseMenuItem(en("git_stage")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if sent.Kind != "git_stage" || !slices.Equal(sent.Paths, []string{"README.md"}) {
			t.Fatalf("sent %+v", sent)
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
	// The history is a graph: each commit's subject and short ID, the names
	// at it, and a row over it that says the rest and takes the click.
	t.Run("History", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		if err := tt.Click(en("git_view_history")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		for _, s := range []string{"Port the Git panel's grouped changes", "9f2c41a", "Merge branch 'split-view'"} {
			if !tt.HasText(s) {
				t.Fatalf("the history has no %q: %q", s, tt.Texts())
			}
		}
		if _, ok := tt.AccessByRole(ui.RoleList, en("git_view_history")); !ok {
			t.Fatalf("the history is not the graph: %q", tt.Texts())
		}
		// At the list's first width the author, the age and the names are
		// left to the tooltip, which also describes the row.
		if row, _ := tt.AccessByRole(ui.RoleOption, "Port the Git panel's grouped changes"); row.Description != "Port the Git panel's grouped changes\n9f2c41ab · Ada Chen · 40 min ago · main" {
			t.Fatalf("the row's tooltip %q", row.Description)
		}
		shot(t, tt, "git-history")
		// A click chooses the commit and reads its changes.
		var read string
		var sent GitAction
		pr.Actions = &Actions{ReadCommit: func(id string) { read = id }, Git: func(_ *ui.Context, a GitAction) { sent = a }}
		pr.SetGitState(GitState{Known: true, Kind: "repository", Head: true})
		tt.Frame()
		merge := pr.Data.Repo.Commits[1]
		mergeRow := func() ui.AccessInfo {
			row, _ := tt.AccessByRole(ui.RoleOption, "Merge branch 'split-view'")
			return row
		}
		clickRole(t, tt, ui.RoleOption, "Merge branch 'split-view'", 0)
		if row := mergeRow(); pr.Git.commitID != merge.ID || read != merge.ID || !row.Selected {
			t.Fatalf("chosen %q, read %q, selected %v", pr.Git.commitID, read, row.Selected)
		}
		// The right-click menu is the system's and acts at the commit.
		menu := func(item string) {
			t.Helper()
			row := mergeRow()
			tt.RightClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
			if err := tt.ChooseMenuItem(en(item)); err != nil {
				t.Fatalf("%v: %q", err, tt.Menu())
			}
			tt.Frame()
			tt.Advance(300 * time.Millisecond)
			tt.Frame()
		}
		menu("git_copy_commit_id")
		if got := tt.Clipboard(); got != merge.ID {
			t.Fatalf("copied %q", got)
		}
		menu("git_cherry_pick")
		if cf := pr.Git.confirm; !cf.open || cf.message != en("git_cherry_pick_confirm") || cf.target != "e5a31c7f Merge branch 'split-view'" {
			t.Fatalf("the confirmation %+v", cf)
		}
		clickRole(t, tt, ui.RoleButton, en("git_cherry_pick"), 0)
		if sent.Kind != "git_cherry_pick" || sent.Target != merge.ID {
			t.Fatalf("sent %+v", sent)
		}
		// The list grows at its end while the repository has more commits.
		more := 0
		pr.Actions.GitHistory = func() { more++ }
		pr.SetGitHistory(GitHistoryState{Next: true})
		tt.Frame()
		clickRole(t, tt, ui.RoleButton, en("files_load_more"), 0)
		pr.SetGitHistory(GitHistoryState{Next: true, Loading: true})
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleButton, en("files_load_more")); more != 1 || ok {
			t.Fatalf("Load more read %d times, shown while reading %v", more, ok)
		}
		pr.SetGitHistory(GitHistoryState{})
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleButton, en("files_load_more")); ok {
			t.Fatal("Load more shows with nothing more to read")
		}
		// A Node that sends no parents has the plain list, with the same rows.
		pr.Data.Repo.Commits[2].Parents = nil
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleList, en("git_view_history")); ok || !tt.HasText("Ada Chen") || !tt.HasText("9f2c41ab") {
			t.Fatalf("no plain list without parents: %q", tt.Texts())
		}
	})
	// A row takes its click in the frame the history first shows: the
	// graph's arrival moves the graph's own rows, not the ones over them.
	t.Run("HistoryClickAtOnce", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		pr.Git.view = 1
		tt.Frame()
		row, ok := tt.AccessByRole(ui.RoleOption, "Merge branch 'split-view'")
		if !ok {
			t.Fatalf("no row in the first frame: %q", tt.Texts())
		}
		tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
		tt.Frame()
		if pr.Git.commitID != pr.Data.Repo.Commits[1].ID {
			t.Fatalf("the click chose %q", pr.Git.commitID)
		}
	})
	// Lanes keep every line the graph draws ending at a dot; the names at
	// a commit are ordered and cut to the room.
	t.Run("HistoryLanes", func(t *testing.T) {
		commit := func(id string, parents ...string) preview.Commit {
			return preview.Commit{ID: id, Parents: append([]string{}, parents...)}
		}
		for _, o := range []struct {
			name    string
			commits []preview.Commit
			want    []int
		}{
			{"line", []preview.Commit{commit("c", "b"), commit("b", "a"), commit("a")}, []int{0, 0, 0}},
			{"merge", []preview.Commit{commit("m", "a", "b"), commit("a", "r"), commit("b", "r"), commit("r")}, []int{0, 0, 1, 0}},
			{"two heads", []preview.Commit{commit("x", "r"), commit("y", "r"), commit("r")}, []int{0, 1, 0}},
			{"parent beyond the page", []preview.Commit{commit("m", "a", "gone"), commit("a", "far"), commit("z", "far")}, []int{0, 0, 1}},
			{"lines that leave the list keep their lanes", []preview.Commit{commit("x", "far"), commit("y", "far"), commit("z")}, []int{0, 1, 2}},
			{"a freed lane is taken again", []preview.Commit{commit("m", "a", "b"), commit("b", "a"), commit("a", "r"), commit("n", "r", "q"), commit("q", "r"), commit("r")}, []int{0, 1, 0, 1, 1, 0}},
		} {
			if got, ok := graphLanes(o.commits); !ok || !slices.Equal(got, o.want) {
				t.Errorf("%s: lanes %v (%v), want %v", o.name, got, ok, o.want)
			}
		}
		if _, ok := graphLanes([]preview.Commit{commit("b", "a"), {ID: "a"}}); ok {
			t.Error("lanes for a commit whose parents are not known")
		}
		refs := []string{"remotes/origin/main", "tags/v1", "heads/topic", "HEAD", "heads/main", "heads/main"}
		if got := refLabels(refs, "main"); !slices.Equal(got, []string{"main", "v1", "topic", "origin/main"}) {
			t.Errorf("names %q", got)
		}
		if got := refLabels([]string{"HEAD", "tags/v1"}, ""); !slices.Equal(got, []string{"HEAD", "v1"}) {
			t.Errorf("detached names %q", got)
		}
		labels := []string{"feature/rate-limit", "v2", "origin/feature/rate-limit"}
		for _, o := range []struct {
			room float32
			most int
			want []string
		}{{400, 3, labels}, {180, 3, []string{"feature/rate-limit", "v2"}}, {210, 3, []string{"feature/rate-limit", "v2", "+1"}},
			{100, 3, []string{"feature/rat…"}}, {50, 3, nil}, {400, 1, []string{"feature/rate-limit", "+2"}}, {140, 1, []string{"feature/rate-limit"}}} {
			if got := fitRefs(labels, o.room, o.most); !slices.Equal(got, o.want) {
				t.Errorf("room %v, at most %d: names %q, want %q", o.room, o.most, got, o.want)
			}
		}
	})
	// A diff's toolbar chooses Unified or Split for every diff of the
	// window.
	t.Run("DiffLayout", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		clickRole(t, tt, ui.RoleTreeItem, "README.md", 0)
		split := false
		tt2 := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			kit.BindDiffSplit(c, &split)
			ui.Column(c).Fill().Children(func() { Page(c, Changes, pr, func() {}) })
		}, 1100, 760, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt2.SetLocale("en")
		tt2.Frame()
		splitters := func() (n int) {
			for _, e := range tt2.AccessAll() {
				if e.Role == ui.RoleSplitter && e.Bounds.X > pr.Git.side+40 {
					n++
				}
			}
			return n
		}
		if n := splitters(); n != 0 {
			t.Fatalf("the diff starts with %d sides divided", n)
		}
		if err := tt2.Click(en("git_diff_split")); err != nil {
			t.Fatal(err)
		}
		tt2.Frame()
		tt2.Frame()
		if n := splitters(); n != 1 || !split {
			t.Fatalf("Split divides %d diffs (%v)", n, split)
		}
		clickRole(t, tt2, ui.RoleButton, en("git_diff"), 0)
		if pr.Git.change != allChanges {
			t.Fatalf("shown %q", pr.Git.change)
		}
		if n := splitters(); n < 2 {
			t.Fatalf("the whole diff divides %d of its files", n)
		}
		if err := tt2.Click(en("git_diff_unified")); err != nil {
			t.Fatal(err)
		}
		tt2.Frame()
		tt2.Frame()
		if n := splitters(); n != 0 || split {
			t.Fatalf("Unified leaves %d diffs divided (%v)", n, split)
		}
	})
	// The review's frames, written only when $SAILRY_SNAPSHOTS names a
	// folder: the demo repository's history as a graph, at the list's first
	// width and widened, and a change's diff unified and split, at 2x in
	// both appearances.
	t.Run("Snapshots", func(t *testing.T) {
		if os.Getenv("SAILRY_SNAPSHOTS") == "" {
			t.Skip("SAILRY_SNAPSHOTS is not set")
		}
		for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
			name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
			pr := NewProjects(preview.Demo("en", "local").Workspace).Of("harbor-api")
			split := false
			tt := ui.NewTesterWith(func(c *ui.Context) {
				theme.Apply(c, "", mode)
				kit.BindDiffSplit(c, &split)
				ui.Column(c).Fill().Background(theme.Of(c).Background).Children(func() { Page(c, Changes, pr, func() {}) })
			}, 1180, 760, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
			tt.SetLocale("en")
			tt.SetScale(2)
			rest := func() {
				tt.Frame()
				tt.Advance(2 * time.Second)
				tt.Frame()
			}
			pr.Git.view, pr.Git.commitID = 1, pr.Data.Repo.Commits[1].ID
			rest()
			shot(t, tt, "git-history-graph-"+name)
			pr.Git.side = sideMax
			rest()
			shot(t, tt, "git-history-graph-wide-"+name)
			// A history longer than one read of the Node (100 commits): a
			// branch merged above the hundredth commit and forked below it,
			// so its lane runs across where the second read begins.
			demo := pr.Data.Repo.Commits
			pr.Data.Repo.Commits = longHistory()
			pr.Git.commitID = ""
			rest()
			list, _ := tt.AccessByRole(ui.RoleListBox, en("git_view_history"))
			tt.Wheel(list.Bounds.X+40, list.Bounds.Y+200, 0, 92*36)
			tt.Move(8, 400)
			rest()
			shot(t, tt, "git-history-graph-continued-"+name)
			pr.Data.Repo.Commits = demo
			pr.Git.side, pr.Git.view = sideWidth, 0
			pr.showChange("internal/booking/reserve.go", "all")
			rest()
			shot(t, tt, "git-diff-unified-"+name)
			split = true
			rest()
			shot(t, tt, "git-diff-split-"+name)
			pr.showChange(allChanges, "all")
			rest()
			shot(t, tt, "git-diff-split-all-"+name)
		}
	})
	// Ctrl+G then Down pulls, which the preview only tells. The key after
	// Ctrl+G completes the chord or drops it: a Down the list took, or one
	// pressed after another key, does not pull later.
	t.Run("Chord", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		press := func(mods ui.Modifiers, key ui.Key) {
			tt.Key(mods, key)
			tt.Frame()
		}
		pulled := func() bool { return tt.HasText(en("preview_notice")) }
		clickRole(t, tt, ui.RoleTreeItem, "docs", 0)
		press(ui.Ctrl, ui.KeyG)
		if !pr.Git.chord {
			t.Fatal("Ctrl+G does not start the chord")
		}
		press(0, ui.KeyDown) // the list's own
		clickRole(t, tt, ui.RoleButton, en("git_expand_commit"), 0)
		press(0, ui.KeyDown)
		if pr.Git.chord || pulled() {
			t.Fatalf("chord %v after the list took its key, pulled %v", pr.Git.chord, pulled())
		}
		press(ui.Ctrl, ui.KeyG)
		press(0, ui.KeyUp)
		press(0, ui.KeyDown)
		if pr.Git.chord || pulled() {
			t.Fatalf("chord %v after another key, pulled %v", pr.Git.chord, pulled())
		}
		press(ui.Ctrl, ui.KeyG)
		press(0, ui.KeyDown)
		if pr.Git.chord || !pulled() {
			t.Fatalf("chord %v, Ctrl+G Down does not pull: %q", pr.Git.chord, tt.Texts())
		}
	})
	// Dropping a stash names it by its message and drops it by its ID.
	t.Run("StashDrop", func(t *testing.T) {
		pr, tt := pageTester(t, Changes)
		var dropped GitAction
		pr.Actions = &Actions{Git: func(_ *ui.Context, a GitAction) { dropped = a }}
		pr.SetGitState(GitState{Known: true, Kind: "repository", Head: true})
		pr.SetGitTargets([]string{"c0ffee"}, nil)
		tt.Frame()
		clickRole(t, tt, ui.RoleMenuButton, en("git_actions"), 0)
		clickMenu(t, tt, en("git_stash_menu"))
		clickMenu(t, tt, en("git_stash_drop"))
		message := pr.Data.Repo.Stashes[0]
		if err := tt.Click(message); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if !pr.Git.confirm.open || pr.Git.confirm.target != message {
			t.Fatalf("the confirmation names %q, not the stash %q: %q", pr.Git.confirm.target, message, tt.Texts())
		}
		clickRole(t, tt, ui.RoleButton, en("git_stash_drop"), 0)
		if dropped.Kind != "git_stash_drop" || dropped.Target != "c0ffee" {
			t.Fatalf("dropped %+v", dropped)
		}
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

// longHistory is 130 commits of a main line with a branch merged at the
// 96th and forked from the 108th: its commits lie on both sides of the
// hundredth, where the Node's second page begins.
func longHistory() []preview.Commit {
	id := func(kind string, n int) string {
		sum := sha1.Sum(fmt.Append(nil, kind, n))
		return hex.EncodeToString(sum[:8])
	}
	var out []preview.Commit
	add := func(hash, message string, refs []string, parents ...string) {
		out = append(out, preview.Commit{ID: hash, Message: message, Author: "Maya Lindqvist", Refs: refs,
			Time: preview.Now.Add(-time.Duration(len(out)+1) * 3 * time.Hour), Parents: parents})
	}
	for n := 0; len(out) < 130; n++ {
		switch {
		case n == 95:
			add(id("a", n), "Merge branch 'feature/export'", nil, id("a", n+1), id("b", 0))
			for k := 0; k < 6; k++ {
				add(id("a", n+1+k), fmt.Sprintf("Tune query %d of the report", k+1), nil, id("a", n+2+k))
				parent := id("b", k+1)
				if k == 5 {
					parent = id("a", n+7)
				}
				var refs []string
				if k == 0 {
					refs = []string{"heads/feature/export"}
				}
				add(id("b", k), fmt.Sprintf("Export step %d", 6-k), refs, parent)
			}
			n += 6
		default:
			add(id("a", n), fmt.Sprintf("Change %d of the service", 130-n), nil, id("a", n+1))
		}
	}
	return out
}
