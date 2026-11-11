package picker

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

func en(key string, args ...any) string { return i18n.In("en", key, args...) }

// bench is a picker in a window of its own, and the host it browses.
type bench struct {
	t  *testing.T
	s  *State
	tt *ui.Tester
}

func newBench(t *testing.T, o Options) *bench {
	t.Helper()
	b := &bench{t: t, s: &State{}}
	b.tt = ui.NewTesterWith(func(c *ui.Context) {
		b.s.View(c)
		kit.ToastHost(c)
	}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	b.tt.SetLocale("en")
	if o.Title == "" {
		o.Title, o.Confirm = "directory_title", "directory_title"
	}
	b.s.Open(o)
	b.tt.Frame()
	b.settle()
	return b
}

func (b *bench) settle() {
	b.tt.Advance(time.Second)
	b.tt.Frame()
}

// control is the topmost control of a role named name.
func (b *bench) control(name string, roles ...ui.Role) (ui.AccessInfo, bool) {
	all := b.tt.AccessAll()
	for i := len(all) - 1; i >= 0; i-- {
		if all[i].Name == name && slices.Contains(roles, all[i].Role) {
			return all[i], true
		}
	}
	return ui.AccessInfo{}, false
}

func (b *bench) button(name string) ui.AccessInfo {
	b.t.Helper()
	e, ok := b.control(name, ui.RoleButton, ui.RoleMenuButton)
	if !ok {
		b.t.Fatalf("no button %q: %q", name, b.tt.Texts())
	}
	return e
}

func (b *bench) press(name string) {
	b.t.Helper()
	e := b.button(name)
	if e.Disabled {
		b.t.Fatalf("the button %q is disabled", name)
	}
	b.tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
	b.settle()
}

// item is an item of the open Kit menu.
func (b *bench) item(name string) ui.AccessInfo {
	b.t.Helper()
	e, ok := b.control(name, ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio, ui.RoleMenuButton)
	if !ok {
		b.t.Fatalf("no menu item %q: %q", name, b.tt.Texts())
	}
	return e
}

func (b *bench) click(text string) {
	b.t.Helper()
	if err := b.tt.Click(text); err != nil {
		b.t.Fatalf("click %q: %v: %q", text, err, b.tt.Texts())
	}
	b.settle()
}

// double clicks the entry named name twice within the double click's time.
func (b *bench) double(name string) {
	b.t.Helper()
	r, ok := b.tt.Find(name)
	if !ok {
		b.t.Fatalf("no entry %q: %q", name, b.tt.Texts())
	}
	b.tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
	b.tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
	b.settle()
}

func (b *bench) row(name string) ui.AccessInfo {
	b.t.Helper()
	e, ok := b.tt.AccessByRole(ui.RoleRow, name)
	if !ok {
		b.t.Fatalf("no row %q: %q", name, b.tt.Texts())
	}
	return e
}

func (b *bench) has(texts ...string) {
	b.t.Helper()
	for _, text := range texts {
		if !b.tt.HasText(text) {
			b.t.Fatalf("the picker does not show %q: %q", text, b.tt.Texts())
		}
	}
}

func (b *bench) lacks(texts ...string) {
	b.t.Helper()
	for _, text := range texts {
		if b.tt.HasText(text) {
			b.t.Fatalf("the picker shows %q: %q", text, b.tt.Texts())
		}
	}
}

// remote is a host whose reads the test fails or holds back.
type remote struct {
	catalog *Catalog
	fail    map[string]string // the problem of a directory, by the path asked
	hold    bool
	held    []func()
	asked   []string
}

func newRemote() *remote { return &remote{catalog: NewCatalog(false), fail: map[string]string{}} }

func (r *remote) source() Source {
	return Source{
		Browse: func(directory string, done func(Listing, string)) {
			r.asked = append(r.asked, directory)
			answer := func() {
				if problem := r.fail[directory]; problem != "" {
					done(Listing{}, problem)
					return
				}
				if directory == "" {
					directory = "/preview"
				}
				done(r.catalog.List(directory))
			}
			if r.hold {
				r.held = append(r.held, answer)
				return
			}
			answer()
		},
		Manage: func(action Action, done func(string)) { done(r.catalog.Apply(action)) },
	}
}

// views are the picker's frames: its layout and what gives way in a small
// window, the sidebar's groups, what each mode lets be chosen, a read that
// fails or hangs, and the keys.
func views(t *testing.T) {
	projects := []Location{{Name: "scratchpad", Path: "/preview/scratch"}, {Name: "sailry", Path: "/preview/sailry"}}

	t.Run("layout", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Host: "build-box", Preview: true, Projects: projects})
		dialog, ok := b.tt.AccessByRole(ui.RoleDialog, en("directory_title"))
		// 1040 by 680 inside its hairline, centered.
		if !ok || dialog.Bounds != (ui.Rect{X: 119, Y: 69, W: 1042, H: 682}) {
			t.Fatalf("the dialog is at %+v", dialog.Bounds)
		}
		// The host is named beside the title, and the sidebar lists the
		// host's places, the projects, then its devices.
		b.has("build-box", en("directory_counts_hidden", "directories", 5, "files", 11, "hidden", 1))
		texts := b.tt.Texts()
		places, registered, devices := slices.Index(texts, en("directory_locations")), slices.Index(texts, en("projects")), slices.Index(texts, en("directory_devices"))
		if places < 0 || registered < places || devices < registered {
			t.Fatalf("the sidebar's groups are at %d %d %d: %q", places, registered, devices, texts)
		}
		// The list is a named table whose columns say how they sort.
		if _, ok := b.tt.AccessByRole(ui.RoleTable, en("files")); !ok {
			t.Fatalf("the entries are no table named %q", en("files"))
		}
		for _, column := range []string{"files_entry_name", "directory_size", "directory_modified", "directory_kind"} {
			if _, ok := b.tt.AccessByRole(ui.RoleColumnHeader, en(column)); !ok {
				t.Fatalf("no column %q: %q", en(column), b.tt.Texts())
			}
		}
		b.click(en("directory_modified"))
		header, _ := b.tt.AccessByRole(ui.RoleColumnHeader, en("directory_modified"))
		first := slices.IndexFunc(b.tt.AccessAll(), func(e ui.AccessInfo) bool {
			return e.Role == ui.RoleRow && e.Name != "" && !e.Disabled && e.Bounds.Y > header.Bounds.Y
		})
		if header.Sort != ui.SortAscending || b.tt.AccessAll()[first].Name != "restricted" {
			t.Fatalf("sorting by Modified gives %v and %q first", header.Sort, b.tt.AccessAll()[first].Name)
		}
		// An idle picker asks for no frame.
		if b.tt.FramePending() {
			t.Fatal("the idle picker asks for another frame")
		}

		// New folder is a button of the header.
		b.press(en("directory_create"))
		b.tt.Type("drafts")
		b.tt.Key(0, ui.KeyEnter)
		b.settle()
		b.has("drafts", en("directory_counts_hidden", "directories", 6, "files", 11, "hidden", 1))

		// A narrower window gives up Kind, then Modified, then the sidebar,
		// whose title and locations move over and into the header.
		for _, size := range []struct {
			width, height int
			shown, gone   []string
		}{
			{1000, 700, []string{en("directory_modified"), en("directory_locations")}, []string{en("directory_kind")}},
			{900, 700, []string{en("directory_size"), en("directory_locations")}, []string{en("directory_kind"), en("directory_modified")}},
			{760, 560, []string{en("files_entry_name"), en("directory_size"), "build-box", en("directory_title")},
				[]string{en("directory_kind"), en("directory_modified"), en("directory_locations")}},
		} {
			b.tt.SetSize(size.width, size.height)
			b.settle()
			b.has(size.shown...)
			b.lacks(size.gone...)
			if dialog, _ := b.tt.AccessByRole(ui.RoleDialog, en("directory_title")); dialog.Bounds.W != float32(size.width-48) {
				t.Fatalf("at %d wide the dialog is %+v", size.width, dialog.Bounds)
			}
		}
		b.press(en("more"))
		for _, name := range []string{"directory_hidden", "directory_refresh", "directory_go_to", "directory_locations", "projects", "directory_devices"} {
			b.item(en(name))
		}
	})

	// The current directory marks the nearest location that holds it, and
	// the projects' group shows only where the caller names some.
	t.Run("places", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true, Projects: projects})
		marked := func() (out []string) {
			for _, e := range b.tt.AccessAll() {
				if e.Role == ui.RoleButton && e.Current == ui.CurrentLocation {
					out = append(out, e.Name)
				}
			}
			return
		}
		if got := marked(); !slices.Equal(got, []string{en("directory_root")}) {
			t.Fatalf("at the root the sidebar marks %q", got)
		}
		b.press("scratchpad")
		b.double("new-project")
		if got := marked(); b.s.listing.Path != "/preview/scratch/new-project" || !slices.Equal(got, []string{"scratchpad"}) {
			t.Fatalf("inside a project at %s the sidebar marks %q", b.s.listing.Path, got)
		}
		bare := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true})
		bare.has(en("directory_locations"), en("directory_devices"))
		bare.lacks(en("projects"))
	})

	t.Run("modes", func(t *testing.T) {
		var chosen []string
		choose := func(paths []string) bool { chosen = paths; return true }
		// A directory picker always has a choice, which Confirm describes:
		// the directory shown, or the one selected in it. Its files show
		// dimmed and take no selection.
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true, Choose: choose})
		confirm := func() ui.AccessInfo { return b.button(en("directory_title")) }
		if c := confirm(); c.Disabled || c.Description != "/preview" {
			t.Fatalf("with nothing selected Confirm is %+v", c)
		}
		b.click("scratch")
		if c := confirm(); c.Disabled || c.Description != "/preview/scratch" || !b.row("scratch").Selected {
			t.Fatalf("with a directory selected Confirm describes %q", c.Description)
		}
		b.click("release-notes.md")
		if c := confirm(); c.Disabled || c.Description != "/preview/scratch" || !b.row("scratch").Selected || b.row("release-notes.md").Selected {
			t.Fatalf("a file took the selection of a directory picker: %q", c.Description)
		}
		b.double("release-notes.md")
		if !b.s.Showing() || b.s.listing.Path != "/preview" || chosen != nil {
			t.Fatalf("a double click on a file did something: %q %v", b.s.listing.Path, chosen)
		}
		// The arrows stop at the last directory rather than on a file.
		b.click("scratch")
		b.tt.Key(0, ui.KeyDown)
		b.settle()
		if b.s.current != "scratch" {
			t.Fatalf("the arrow moved the selection to %q", b.s.current)
		}
		b.press(en("directory_title"))
		if !slices.Equal(chosen, []string{"/preview/scratch"}) || b.s.Showing() {
			t.Fatalf("Confirm chose %q", chosen)
		}

		// A file picker says that nothing is chosen, then names the file.
		chosen = nil
		b = newBench(t, Options{Mode: File, Title: "files_open", Confirm: "directory_open", Source: NewCatalog(false).Source(), Preview: true, Choose: choose})
		b.has(en("directory_nothing_selected"))
		if b.button(en("directory_open")).Disabled != true {
			t.Fatal("a file picker confirms with no file")
		}
		b.click("scratch")
		if !b.button(en("directory_open")).Disabled || !b.tt.HasText(en("directory_nothing_selected")) {
			t.Fatal("a file picker confirms a directory")
		}
		b.click("banner.png")
		if c := b.button(en("directory_open")); c.Disabled || c.Description != "/preview/banner.png" {
			t.Fatalf("with a file selected Confirm is %+v", c)
		}
		b.double("release-notes.md")
		if !slices.Equal(chosen, []string{"/preview/release-notes.md"}) || b.s.Showing() {
			t.Fatalf("a double click chose %q", chosen)
		}

		// A picker of several files counts them instead of listing paths.
		chosen = nil
		b = newBench(t, Options{Mode: Files, Title: "files_open", Confirm: "directory_open", Source: NewCatalog(false).Source(), Preview: true, Choose: choose})
		b.has(en("directory_nothing_selected"))
		b.click("banner.png")
		if c := b.button(en("directory_open")); c.Disabled || c.Description != "/preview/banner.png" {
			t.Fatalf("with one file chosen Confirm is %+v", c)
		}
		if err := b.tt.ClickWith(ui.Cmd, "release-notes.md"); err != nil {
			t.Fatal(err)
		}
		b.settle()
		b.has(en("directory_selected_files", "count", 2))
		b.lacks("/preview/banner.png")
		b.press(en("directory_open"))
		if !slices.Equal(chosen, []string{"/preview/banner.png", "/preview/release-notes.md"}) {
			t.Fatalf("Confirm chose %q", chosen)
		}
	})

	t.Run("failed read", func(t *testing.T) {
		// A first path that cannot show gives way to the host's home, and a
		// toast names it.
		host := newRemote()
		host.fail["/gone"] = "directory_missing"
		b := newBench(t, Options{Mode: Directory, Source: host.source(), Host: "build-box", Initial: "/gone"})
		b.has(en("directory_fallback", "path", "/gone"), "scratch")
		if !slices.Equal(host.asked, []string{"/gone", ""}) || b.s.listing.Path != "/preview" {
			t.Fatalf("the picker asked for %q and shows %q", host.asked, b.s.listing.Path)
		}
		// A later failure keeps the directory shown, says why, and leaves
		// Confirm with its choice.
		host.fail["/preview/restricted"] = "directory_denied"
		b.double("restricted")
		b.has(en("directory_denied"), "scratch")
		if b.s.listing.Path != "/preview" || b.button(en("directory_title")).Disabled {
			t.Fatalf("a failed read left %q", b.s.listing.Path)
		}

		// With nothing to show, the reason takes the entries' place with
		// Retry; the address, Go to folder and the sidebar still lead out.
		host = newRemote()
		host.fail[""] = "directory_host_unavailable"
		b = newBench(t, Options{Mode: Directory, Source: host.source(), Host: "build-box", Projects: projects})
		b.has(en("directory_host_unavailable"), "build-box", "scratchpad")
		if !b.button(en("directory_title")).Disabled || b.button(en("settings_retry")).Disabled || b.button(en("directory_address")).Disabled ||
			b.button(en("directory_home")).Disabled || b.button(en("directory_create")).Disabled != true {
			t.Fatalf("the failed picker's controls are wrong: %q", b.tt.Texts())
		}
		b.press(en("more"))
		if b.item(en("directory_go_to")).Disabled || b.item(en("directory_refresh")).Disabled {
			t.Fatal("Go to folder or Refresh is disabled after a failed read")
		}
		b.tt.Key(0, ui.KeyEscape)
		b.settle()
		b.press(en("settings_retry"))
		b.has(en("directory_host_unavailable"))
		delete(host.fail, "")
		b.press(en("settings_retry"))
		b.has("scratch", "many")
		b.lacks(en("directory_host_unavailable"), en("settings_retry"))
		if b.button(en("directory_title")).Disabled {
			t.Fatal("Confirm stays disabled after the retry")
		}

		// A home that fails as well leaves the directory asked for with its
		// own reason and no word of a home; Retry opens again.
		host = newRemote()
		host.fail["/gone"], host.fail[""] = "directory_missing", "directory_host_unavailable"
		b = newBench(t, Options{Mode: Directory, Source: host.source(), Initial: "/gone"})
		b.has(en("directory_missing"), "/gone")
		b.lacks(en("directory_fallback", "path", "/gone"), en("directory_host_unavailable"))
		delete(host.fail, "")
		b.press(en("settings_retry"))
		b.has(en("directory_fallback", "path", "/gone"), "scratch")
		if b.s.listing.Path != "/preview" {
			t.Fatalf("the retry shows %q", b.s.listing.Path)
		}

		// The path field takes another directory from the failed state.
		host = newRemote()
		host.fail[""] = "directory_failed"
		b = newBench(t, Options{Mode: Directory, Source: host.source()})
		b.has(en("directory_failed"))
		b.press(en("directory_address"))
		b.tt.Type("/preview/scratch")
		b.tt.Key(0, ui.KeyEnter)
		b.settle()
		b.has("new-project")
		if b.s.listing.Path != "/preview/scratch" {
			t.Fatalf("the address led to %q", b.s.listing.Path)
		}
	})

	t.Run("loading", func(t *testing.T) {
		// A read that hangs shows, and blocks nothing: another navigation
		// replaces it and its late answer is dropped.
		host := newRemote()
		host.hold = true
		b := newBench(t, Options{Mode: Directory, Source: host.source(), Projects: projects})
		busy := func() bool {
			return slices.ContainsFunc(b.tt.AccessAll(), func(e ui.AccessInfo) bool { return e.Busy })
		}
		if !busy() || !b.tt.HasText(en("plugins_loading")) || !b.button(en("directory_title")).Disabled {
			t.Fatalf("the first read does not show as loading: %q", b.tt.Texts())
		}
		b.press("sailry")
		if len(host.held) != 2 || !slices.Equal(host.asked, []string{"", "/preview/sailry"}) {
			t.Fatalf("the hung read blocked the sidebar: %q", host.asked)
		}
		host.held[1]()
		b.settle()
		b.has("main.go")
		if busy() || b.tt.FramePending() || b.s.listing.Path != "/preview/sailry" {
			t.Fatalf("the read that arrived still shows as loading at %q", b.s.listing.Path)
		}
		host.held[0]()
		b.settle()
		if b.s.listing.Path != "/preview/sailry" || len(b.s.history) != 1 {
			t.Fatalf("a stale answer showed %q with the history %q", b.s.listing.Path, b.s.history)
		}
		// Over a directory that shows, the read keeps its entries and the
		// header's controls, and holds Confirm back.
		b.double("src")
		b.has("main.go")
		if !busy() || !b.button(en("directory_title")).Disabled || b.button(en("directory_up")).Disabled || b.button(en("directory_create")).Disabled {
			t.Fatalf("the read over a directory is wrong: %q", b.tt.Texts())
		}
		b.press(en("directory_up"))
		host.held[3]()
		b.settle()
		host.held[2]()
		b.settle()
		if busy() || b.s.listing.Path != "/preview" || b.button(en("directory_title")).Disabled {
			t.Fatalf("navigating away from a hung read left %q", b.s.listing.Path)
		}
	})

	t.Run("keys", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true})
		// The list has the focus from the start: the arrows select, Enter
		// opens, Backspace and Cmd+Up go up, typing finds an entry.
		b.tt.Key(0, ui.KeyDown)
		b.settle()
		if b.s.current != "empty" {
			t.Fatalf("the first arrow selected %q", b.s.current)
		}
		b.tt.Key(0, ui.KeyS)
		b.tt.Key(0, ui.KeyC)
		b.settle()
		if b.s.current != "scratch" {
			t.Fatalf("typing selected %q", b.s.current)
		}
		b.tt.Key(0, ui.KeyEnter)
		b.settle()
		b.tt.Key(0, ui.KeyDown)
		b.tt.Key(0, ui.KeyEnter)
		b.settle()
		if b.s.listing.Path != "/preview/scratch/new-project" {
			t.Fatalf("Enter opened %q", b.s.listing.Path)
		}
		b.tt.Key(0, ui.KeyBackspace)
		b.settle()
		if b.s.listing.Path != "/preview/scratch" {
			t.Fatalf("Backspace went to %q", b.s.listing.Path)
		}
		b.tt.Key(ui.Cmd, ui.KeyUp)
		b.settle()
		if b.s.listing.Path != "/preview" {
			t.Fatalf("Cmd+Up went to %q", b.s.listing.Path)
		}
		// Back and Forward walk the dialog's own history.
		b.press(en("directory_back"))
		b.press(en("directory_back"))
		if b.s.listing.Path != "/preview/scratch/new-project" || b.button(en("directory_forward")).Disabled {
			t.Fatalf("Back went to %q", b.s.listing.Path)
		}
		b.press(en("directory_forward"))
		if b.s.listing.Path != "/preview/scratch" {
			t.Fatalf("Forward went to %q", b.s.listing.Path)
		}
		// Cmd+Shift+G asks for a path; Escape clears it, closes it, then
		// closes the picker. The pointer rests on no button, whose tip
		// Escape would close first.
		b.tt.Move(4, 4)
		b.tt.Key(ui.Cmd|ui.Shift, ui.KeyG)
		b.settle()
		if !b.s.jump.open || b.s.jump.query != "/preview/scratch" {
			t.Fatalf("Cmd+Shift+G did not ask for a path: %+v", b.s.jump)
		}
		b.tt.Key(0, ui.KeyEscape)
		b.settle()
		b.tt.Key(0, ui.KeyEscape)
		b.settle()
		if b.s.jump.open || !b.s.Showing() {
			t.Fatalf("Escape did not close Go to folder alone: %v %v", b.s.jump.open, b.s.Showing())
		}
		if list, _ := b.tt.AccessByRole(ui.RoleTable, en("files")); !list.Focused {
			t.Fatal("the entries did not take the focus back from Go to folder")
		}
		b.tt.Key(0, ui.KeyEscape)
		b.settle()
		if b.s.Showing() {
			t.Fatal("Escape did not close the picker")
		}
	})

	// A long name in the grid keeps its start and its end on two lines,
	// and the entry's menu is the host's own, with New folder and the trash.
	t.Run("grid and menus", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true})
		b.press(en("directory_grid"))
		long := "WorkBuddy-linux-x64-deb-5.6.2.40046327-c7b525f3.deb"
		name, ok := b.tt.AccessByRole(ui.RoleText, long)
		if !ok || name.Description != long || name.Bounds.H > 40 || name.Bounds.W > 130 {
			t.Fatalf("the grid's long name is %+v, described %q", name.Bounds, name.Description)
		}
		b.press(en("directory_list"))
		if err := b.tt.RightClick("scratch"); err != nil {
			t.Fatal(err)
		}
		menu := b.tt.Menu()
		for _, item := range []string{"directory_open", "files_rename", "directory_create", "directory_trash"} {
			if !slices.Contains(menu, en(item)) {
				t.Fatalf("the entry's menu is %q", menu)
			}
		}
		if err := b.tt.ChooseMenuItem(en("directory_trash")); err != nil {
			t.Fatal(err)
		}
		b.settle()
		b.has(en("directory_trash_confirm", "name", "scratch"))
	})
}
