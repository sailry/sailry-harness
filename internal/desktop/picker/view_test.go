package picker

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func en(key string, args ...any) string { return i18n.In("en", key, args...) }

// bench is a picker in a window of its own, and the host it browses.
type bench struct {
	t  *testing.T
	s  *State
	tt *ui.Tester
	// demo shows the frames as the demo's; palette is the frame's.
	demo    bool
	palette *theme.Palette
}

func newBench(t *testing.T, o Options) *bench {
	t.Helper()
	b := &bench{t: t, s: &State{}}
	b.tt = ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, theme.BuiltinID, theme.Light)
		kit.SetDemo(c, b.demo)
		b.palette = theme.Of(c)
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

// pixel is the color of the last frame at a point, in DIPs.
func (b *bench) pixel(x, y float32) ui.Color {
	c := b.tt.Image().RGBAAt(int(x), int(y))
	return ui.Color{R: c.R, G: c.G, B: c.B, A: c.A}
}

// near reports whether two opaque colors are the same within the rounding of
// a blend.
func near(a, b ui.Color) bool {
	apart := func(x, y uint8) bool { return x > y+2 || y > x+2 }
	return !apart(a.R, b.R) && !apart(a.G, b.G) && !apart(a.B, b.B)
}

// text is the box of the first text reading s, which Find gives to what s
// labels as well.
func (b *bench) text(s string) ui.Rect {
	b.t.Helper()
	e, ok := b.tt.AccessByRole(ui.RoleText, s)
	if !ok {
		b.t.Fatalf("no text %q: %q", s, b.tt.Texts())
	}
	return e.Bounds
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
					directory = "/Users/alex"
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
	projects := []Location{{Name: "scratchpad", Path: "/Users/alex/scratch"}, {Name: "sailry", Path: "/Users/alex/sailry"}}

	t.Run("layout", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Host: "build-box", Preview: true, Projects: projects})
		dialog, ok := b.tt.AccessByRole(ui.RoleDialog, en("directory_title"))
		// 1040 by 680 inside its hairline, centered.
		if !ok || dialog.Bounds != (ui.Rect{X: 119, Y: 69, W: 1042, H: 682}) {
			t.Fatalf("the dialog is at %+v", dialog.Bounds)
		}
		// This device is not named, and the sidebar lists the host's
		// places, the projects, then its devices.
		b.lacks("build-box")
		b.has(en("directory_counts_hidden", "directories", 8, "files", 11, "hidden", 1))
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
			return e.Role == ui.RoleRow && e.Name != "" && !e.Disabled && !strings.Contains(e.Name, en("directory_modified"))
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
		b.has("drafts", en("directory_counts_hidden", "directories", 9, "files", 11, "hidden", 1))

		// A narrower window gives up Kind, then Modified, then the sidebar,
		// whose title and locations move over and into the header.
		for _, size := range []struct {
			width, height int
			shown, gone   []string
		}{
			{1000, 700, []string{en("directory_modified"), en("directory_locations")}, []string{en("directory_kind")}},
			{900, 700, []string{en("directory_size"), en("directory_locations")}, []string{en("directory_kind"), en("directory_modified")}},
			{760, 560, []string{en("files_entry_name"), en("directory_size"), en("directory_title")},
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
	t.Run("a sort keeps the place", func(t *testing.T) {
		b := newBench(t, Options{Mode: File, Source: NewCatalog(false).Source()})
		table, ok := b.tt.AccessByRole(ui.RoleTable, en("files"))
		if !ok {
			t.Fatalf("no table: %q", b.tt.Texts())
		}
		whole := func(name string) bool {
			row, ok := b.tt.AccessByRole(ui.RoleRow, name)
			return ok && row.Bounds.Y >= table.Bounds.Y && row.Bounds.Y+row.Bounds.H <= table.Bounds.Y+table.Bounds.H+0.5
		}
		name := func(row int) string { return b.s.listing.Entries[b.s.rows[row]].Name }
		last := len(b.s.rows) - 1
		if top, end := name(0), name(last); !whole(top) || whole(end) {
			t.Fatalf("the fixture's entries all show, or none: %q %v, %q %v", top, whole(top), end, whole(end))
		}
		// Nothing is chosen: a sort shows the top of the new order, not
		// where the old order's rows went.
		b.tt.Wheel(table.Bounds.X+table.Bounds.W/2, table.Bounds.Y+table.Bounds.H/2, 0, 4000)
		b.settle()
		if top := name(0); whole(top) {
			t.Fatalf("the list did not scroll: %q", top)
		}
		b.click(en("files_entry_name"))
		if top := name(0); b.s.sort.Column != "name" || !whole(top) {
			t.Fatalf("sorted %+v with nothing chosen, the top %q shows %v", b.s.sort, top, whole(top))
		}
		// The chosen file stays in view when a sort moves it to the end.
		first := slices.IndexFunc(b.s.rows, func(i int) bool { return b.s.listing.Entries[i].Kind == "file" })
		chosen := name(first)
		b.click(chosen)
		b.click(en("files_entry_name"))
		if b.s.current != chosen || b.s.row != last || !whole(chosen) {
			t.Fatalf("after the sort %q is chosen at row %d of %d and %q shows %v", b.s.current, b.s.row, last, chosen, whole(chosen))
		}
	})
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
		if got := marked(); !slices.Equal(got, []string{en("directory_home")}) {
			t.Fatalf("at the home the sidebar marks %q", got)
		}
		b.press("scratchpad")
		b.double("new-project")
		if got := marked(); b.s.listing.Path != "/Users/alex/scratch/new-project" || !slices.Equal(got, []string{"scratchpad"}) {
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
		if c := confirm(); c.Disabled || c.Description != "/Users/alex" {
			t.Fatalf("with nothing selected Confirm is %+v", c)
		}
		b.click("scratch")
		if c := confirm(); c.Disabled || c.Description != "/Users/alex/scratch" || !b.row("scratch").Selected {
			t.Fatalf("with a directory selected Confirm describes %q", c.Description)
		}
		b.click("release-notes.md")
		if c := confirm(); c.Disabled || c.Description != "/Users/alex/scratch" || !b.row("scratch").Selected || b.row("release-notes.md").Selected {
			t.Fatalf("a file took the selection of a directory picker: %q", c.Description)
		}
		b.double("release-notes.md")
		if !b.s.Showing() || b.s.listing.Path != "/Users/alex" || chosen != nil {
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
		if !slices.Equal(chosen, []string{"/Users/alex/scratch"}) || b.s.Showing() {
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
		if c := b.button(en("directory_open")); c.Disabled || c.Description != "/Users/alex/banner.png" {
			t.Fatalf("with a file selected Confirm is %+v", c)
		}
		b.double("release-notes.md")
		if !slices.Equal(chosen, []string{"/Users/alex/release-notes.md"}) || b.s.Showing() {
			t.Fatalf("a double click chose %q", chosen)
		}

		// A picker of several files counts them instead of listing paths.
		chosen = nil
		b = newBench(t, Options{Mode: Files, Title: "files_open", Confirm: "directory_open", Source: NewCatalog(false).Source(), Preview: true, Choose: choose})
		b.has(en("directory_nothing_selected"))
		b.click("banner.png")
		if c := b.button(en("directory_open")); c.Disabled || c.Description != "/Users/alex/banner.png" {
			t.Fatalf("with one file chosen Confirm is %+v", c)
		}
		if err := b.tt.ClickWith(ui.Cmd, "release-notes.md"); err != nil {
			t.Fatal(err)
		}
		b.settle()
		b.has(en("directory_selected_files", "count", 2))
		b.lacks("/Users/alex/banner.png")
		b.press(en("directory_open"))
		if !slices.Equal(chosen, []string{"/Users/alex/banner.png", "/Users/alex/release-notes.md"}) {
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
		if !slices.Equal(host.asked, []string{"/gone", ""}) || b.s.listing.Path != "/Users/alex" {
			t.Fatalf("the picker asked for %q and shows %q", host.asked, b.s.listing.Path)
		}
		// A later failure keeps the directory shown, says why, and leaves
		// Confirm with its choice.
		host.fail["/Users/alex/restricted"] = "directory_denied"
		b.double("restricted")
		b.has(en("directory_denied"), "scratch")
		if b.s.listing.Path != "/Users/alex" || b.button(en("directory_title")).Disabled {
			t.Fatalf("a failed read left %q", b.s.listing.Path)
		}

		// With nothing to show, the reason takes the entries' place with
		// Retry; the address, Go to folder and the sidebar still lead out.
		host = newRemote()
		host.fail[""] = "directory_host_unavailable"
		b = newBench(t, Options{Mode: Directory, Source: host.source(), Host: "build-box", Remote: true, Projects: projects})
		b.has(en("directory_host_unavailable"), en("directory_locations")+" · build-box", "scratchpad")
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
		if b.s.listing.Path != "/Users/alex" {
			t.Fatalf("the retry shows %q", b.s.listing.Path)
		}

		// The path field takes another directory from the failed state.
		host = newRemote()
		host.fail[""] = "directory_failed"
		b = newBench(t, Options{Mode: Directory, Source: host.source()})
		b.has(en("directory_failed"))
		b.press(en("directory_address"))
		b.tt.Type("/Users/alex/scratch")
		b.tt.Key(0, ui.KeyEnter)
		b.settle()
		b.has("new-project")
		if b.s.listing.Path != "/Users/alex/scratch" {
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
		if len(host.held) != 2 || !slices.Equal(host.asked, []string{"", "/Users/alex/sailry"}) {
			t.Fatalf("the hung read blocked the sidebar: %q", host.asked)
		}
		host.held[1]()
		b.settle()
		b.has("main.go")
		if busy() || b.tt.FramePending() || b.s.listing.Path != "/Users/alex/sailry" {
			t.Fatalf("the read that arrived still shows as loading at %q", b.s.listing.Path)
		}
		host.held[0]()
		b.settle()
		if b.s.listing.Path != "/Users/alex/sailry" || len(b.s.history) != 1 {
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
		if busy() || b.s.listing.Path != "/Users/alex" || b.button(en("directory_title")).Disabled {
			t.Fatalf("navigating away from a hung read left %q", b.s.listing.Path)
		}
	})

	t.Run("keys", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true})
		// The list has the focus from the start: the arrows select, Enter
		// opens, Backspace and Cmd+Up go up, typing finds an entry.
		b.tt.Key(0, ui.KeyDown)
		b.settle()
		if b.s.current != "Desktop" {
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
		if b.s.listing.Path != "/Users/alex/scratch/new-project" {
			t.Fatalf("Enter opened %q", b.s.listing.Path)
		}
		b.tt.Key(0, ui.KeyBackspace)
		b.settle()
		if b.s.listing.Path != "/Users/alex/scratch" {
			t.Fatalf("Backspace went to %q", b.s.listing.Path)
		}
		b.tt.Key(ui.Cmd, ui.KeyUp)
		b.settle()
		if b.s.listing.Path != "/Users/alex" {
			t.Fatalf("Cmd+Up went to %q", b.s.listing.Path)
		}
		// Back and Forward walk the dialog's own history.
		b.press(en("directory_back"))
		b.press(en("directory_back"))
		if b.s.listing.Path != "/Users/alex/scratch/new-project" || b.button(en("directory_forward")).Disabled {
			t.Fatalf("Back went to %q", b.s.listing.Path)
		}
		b.press(en("directory_forward"))
		if b.s.listing.Path != "/Users/alex/scratch" {
			t.Fatalf("Forward went to %q", b.s.listing.Path)
		}
		// Cmd+Shift+G asks for a path; Escape clears it, closes it, then
		// closes the picker. The pointer rests on no button, whose tip
		// Escape would close first.
		b.tt.Move(4, 4)
		b.tt.Key(ui.Cmd|ui.Shift, ui.KeyG)
		b.settle()
		if !b.s.jump.open || b.s.jump.query != "/Users/alex/scratch" {
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

	// The chosen states are the neutral selected fill, a step over the
	// hover's, never the emphasis color: the view in use, the sidebar's
	// place and the chosen entry.
	t.Run("selection colors", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source()})
		p := b.palette
		// over is the fill as it shows over what is beside the control.
		check := func(what string, got, around ui.Color) {
			t.Helper()
			if want := p.Selected().Over(around); !near(got, want) || near(got, around) {
				t.Fatalf("%s is filled with %+v over %+v, want the neutral %+v", what, got, around, want)
			}
			if near(got, p.Primary.Over(around)) || near(got, p.Primary) {
				t.Fatalf("%s is filled with the emphasis color %+v", what, got)
			}
		}
		b.tt.Move(4, 4)
		b.settle()
		list, grid := b.button(en("directory_list")).Bounds, b.button(en("directory_grid")).Bounds
		// A point inside each button's fill, beside its glyph.
		in := func(r ui.Rect) ui.Color { return b.pixel(r.X+3, r.Y+r.H/2) }
		if !b.button(en("directory_list")).Checked || b.button(en("directory_grid")).Checked {
			t.Fatal("the list view is not announced as the one in use")
		}
		check("the view in use", in(list), in(grid))
		b.press(en("directory_grid"))
		b.tt.Move(4, 4)
		b.settle()
		check("the grid view in use", in(grid), in(list))
		b.press(en("directory_list"))

		// The place that holds the directory, against the one below it; the
		// place under the pointer is fainter.
		home, desktop := b.button(en("directory_home")).Bounds, b.button(en("directory_desktop")).Bounds
		end := func(r ui.Rect) ui.Color { return b.pixel(r.X+r.W-6, r.Y+r.H/2) }
		b.tt.Move(4, 4)
		b.settle()
		plain := end(desktop)
		check("the current place", end(home), plain)
		b.tt.Move(desktop.X+12, desktop.Y+desktop.H/2)
		b.settle()
		if hover := end(desktop); !near(hover, p.Hover.Over(plain)) || near(hover, end(home)) {
			t.Fatalf("the hovered place is %+v, the current one %+v", hover, end(home))
		}

		// A chosen row, against the row below it.
		b.click("scratch")
		b.tt.Move(4, 4)
		b.settle()
		chosen, other := b.row("scratch").Bounds, b.row("restricted").Bounds
		if !b.row("scratch").Selected {
			t.Fatal("the row is not chosen")
		}
		check("the chosen row", end(chosen), end(other))
		// Several chosen files, and a chosen tile of the grid.
		b = newBench(t, Options{Mode: Files, Title: "files_open", Confirm: "directory_open", Source: NewCatalog(false).Source()})
		b.click("banner.png")
		if err := b.tt.ClickWith(ui.Cmd, "release-notes.md"); err != nil {
			t.Fatal(err)
		}
		b.tt.Move(4, 4)
		b.settle()
		for _, name := range []string{"banner.png", "release-notes.md"} {
			check("the chosen file "+name, end(b.row(name).Bounds), end(b.row("scratch").Bounds))
		}
		b.press(en("directory_grid"))
		b.click("banner.png")
		b.tt.Move(4, 4)
		b.settle()
		tile, ok := b.tt.Find("banner.png")
		beside, found := b.tt.Find("architecture-review.pdf")
		if !ok || !found {
			t.Fatalf("the grid lacks its tiles: %q", b.tt.Texts())
		}
		// Above the name, beside the entry's icon.
		// A corner of each cell, clear of the entry's icon and its name: the
		// selected fill over the faint one MyGo's grid gives its chosen cell.
		got, around := b.pixel(tile.X+10, tile.Y+10), b.pixel(beside.X+10, beside.Y+10)
		if want := p.Selected().Over(p.Selected().Alpha(0.2).Over(around)); !near(got, want) || near(got, p.Primary) {
			t.Fatalf("the chosen tile is filled with %+v over %+v, want the neutral %+v", got, around, want)
		}
	})

	// The address is a field only while it has the focus: a click anywhere
	// else and Escape show the crumbs again and drop what was typed, and
	// that Escape leaves the dialog open.
	t.Run("address", func(t *testing.T) {
		home := NewCatalog(false).Home()
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source()})
		crumbs := func() bool {
			_, ok := b.control(home, ui.RoleMenuButton)
			return ok
		}
		edit := func(text string) {
			t.Helper()
			b.press(en("directory_address"))
			if active, _ := b.tt.TextInputActive(); !b.s.editing || !active || crumbs() {
				t.Fatalf("the address is not a field with the focus: %v %v", b.s.editing, active)
			}
			b.tt.Type(text)
			b.settle()
			if !strings.HasSuffix(b.s.address, text) {
				t.Fatalf("typing left the address %q", b.s.address)
			}
		}
		shown := func(how string) {
			t.Helper()
			if b.s.editing || b.s.address != home || !crumbs() || b.s.listing.Path != home || !b.s.Showing() {
				t.Fatalf("%s left the address editing=%v at %q, the picker at %q", how, b.s.editing, b.s.address, b.s.listing.Path)
			}
		}
		// A click on an entry ends the edit and still chooses the entry.
		edit("/nowhere")
		b.click("scratch")
		shown("a click on an entry")
		if b.s.current != "scratch" {
			t.Fatalf("the click that ended the edit chose %q", b.s.current)
		}
		// A click on the sidebar's empty space, and on the footer's.
		edit("zz")
		places, _ := b.tt.Find(en("directory_devices"))
		b.tt.ClickAt(places.X+60, 700)
		b.settle()
		shown("a click in the sidebar")
		edit("zz")
		cancel := b.button(en("settings_cancel")).Bounds
		b.tt.ClickAt(cancel.X-200, cancel.Y+cancel.H/2)
		b.settle()
		shown("a click in the footer")
		// A click on another control does what it does, and ends the edit.
		edit("zz")
		b.press(en("directory_grid"))
		shown("a click on the view switch")
		if !b.s.Grid {
			t.Fatal("the click that ended the edit did not switch the view")
		}
		b.press(en("directory_list"))
		// Another app taking the window's place does not end it.
		edit("zz")
		b.tt.SetWindowActive(false)
		b.settle()
		b.tt.SetWindowActive(true)
		b.settle()
		if !b.s.editing || !strings.HasSuffix(b.s.address, "zz") {
			t.Fatal("the window going inactive ended the edit")
		}
		// Escape leaves the field, the picker staying open with the keys in
		// its entries; the next Escape closes it. The pointer rests on no
		// button, whose tip Escape would close first.
		b.tt.Move(4, 4)
		b.tt.Key(0, ui.KeyEscape)
		b.settle()
		shown("Escape")
		b.tt.Key(0, ui.KeyUp)
		b.settle()
		if b.s.current != "sailry" {
			t.Fatalf("the entries did not take the keys back from the address: %q is chosen", b.s.current)
		}
		// Enter goes where the field says, from Cmd+L as from a click.
		b.tt.Key(ui.Cmd, ui.KeyL)
		b.settle()
		if active, _ := b.tt.TextInputActive(); !b.s.editing || !active {
			t.Fatal("Cmd+L did not open the address")
		}
		b.tt.Type("/scratch")
		b.tt.Key(0, ui.KeyEnter)
		b.settle()
		if b.s.editing || b.s.listing.Path != home+"/scratch" || !b.tt.HasText("new-project") {
			t.Fatalf("Enter led to %q", b.s.listing.Path)
		}
		b.tt.Key(0, ui.KeyEscape)
		b.settle()
		if b.s.Showing() {
			t.Fatal("Escape outside the address did not close the picker")
		}
	})

	// The title is one line saying what is chosen, level with the header
	// bar; only a host other than this device is named.
	t.Run("heading", func(t *testing.T) {
		for _, c := range []struct {
			mode  Mode
			title string
		}{{Directory, "directory_title"}, {File, "plugins_package"}, {Files, "files_open"}} {
			b := newBench(t, Options{Mode: c.mode, Title: c.title, Confirm: "directory_open", Source: NewCatalog(false).Source(), Host: "this-mac.local"})
			b.lacks("this-mac.local")
			title := b.text(en(c.title))
			back := b.button(en("directory_back")).Bounds
			if apart := title.Y + title.H/2 - back.Y - back.H/2; title.H > 26 || apart < -1 || apart > 1 {
				t.Fatalf("the title %q is at %+v, Back at %+v", en(c.title), title, back)
			}
			// Nothing else shares the title's band of the sidebar.
			places, _ := b.tt.Find(en("directory_locations"))
			for _, e := range b.tt.AccessAll() {
				if e.Role == ui.RoleText && e.Bounds.X < back.X-20 && e.Bounds.Y < places.Y-1 && e.Name != en(c.title) && e.Bounds.W > 0 {
					t.Fatalf("%q shows beside the title at %+v", e.Name, e.Bounds)
				}
			}
		}
		// A remote host is named after the first group's heading, and after
		// the title once the sidebar gives way.
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(true).Source(), Host: "build-box", Remote: true})
		b.has(en("directory_locations") + " · build-box")
		if texts := b.tt.Texts(); slices.Index(texts, en("directory_locations")+" · build-box") < slices.Index(texts, en("directory_title")) {
			t.Fatalf("the host is named before the title: %q", texts)
		}
		b.tt.SetSize(760, 560)
		b.settle()
		title := b.text(en("directory_title"))
		host, ok := b.tt.Find("build-box")
		if apart := host.Y + host.H/2 - title.Y - title.H/2; !ok || apart < -2 || apart > 2 || host.X < title.X+title.W {
			t.Fatalf("without the sidebar the host is at %+v, the title at %+v", host, title)
		}
		// The title stays below the room of macOS's window controls at the
		// app's (12, 16), in the smallest window, where the dialog comes
		// nearest the corner.
		b.tt.SetTitleBar(ui.TitleBar{Height: 46, Left: 80})
		b.settle()
		if title := b.text(en("directory_title")); title.Y < 46-16 {
			t.Fatalf("the title at %+v is under the window controls", title)
		}
	})

	// The sidebar lists the places and the devices its host reports, in the
	// host's order under the desktop's names, and each leads to its path.
	t.Run("locations", func(t *testing.T) {
		catalog := NewCatalog(false)
		home := catalog.Home()
		b := newBench(t, Options{Mode: Directory, Source: catalog.Source()})
		// The sidebar's buttons from the top, by their group.
		listed := func() (out []string) {
			devices, _ := b.tt.Find(en("directory_devices"))
			back := b.button(en("directory_back")).Bounds
			var places []ui.AccessInfo
			for _, e := range b.tt.AccessAll() {
				if e.Role == ui.RoleButton && e.Bounds.X < back.X-20 && e.Bounds.W > 100 {
					places = append(places, e)
				}
			}
			slices.SortFunc(places, func(x, y ui.AccessInfo) int { return int(x.Bounds.Y - y.Bounds.Y) })
			for _, e := range places {
				group := "place"
				if devices.H > 0 && e.Bounds.Y > devices.Y {
					group = "device"
				}
				out = append(out, group+" "+e.Name)
			}
			return out
		}
		want := []string{"place " + en("directory_home"), "place " + en("directory_desktop"), "place " + en("directory_documents"),
			"place " + en("directory_downloads"), "place " + en("directory_applications"), "device Macintosh HD", "device Backup"}
		if got := listed(); !slices.Equal(got, want) {
			t.Fatalf("the sidebar lists %q, want %q", got, want)
		}
		for name, path := range map[string]string{en("directory_downloads"): home + "/Downloads", en("directory_applications"): "/Applications",
			"Backup": "/Volumes/Backup", "Macintosh HD": "/", en("directory_desktop"): home + "/Desktop", en("directory_home"): home} {
			b.press(name)
			if b.s.listing.Path != path {
				t.Fatalf("%q led to %q, want %q", name, b.s.listing.Path, path)
			}
			if place := b.button(name); place.Current != ui.CurrentLocation {
				t.Fatalf("%q is not marked at %q", name, path)
			}
		}
		// A volume names the first crumb of the paths on it.
		b.press("Backup")
		b.double("Archive")
		if _, ok := b.control("/Volumes/Backup", ui.RoleMenuButton); !ok || b.s.listing.Path != "/Volumes/Backup/Archive" {
			t.Fatalf("the volume is no crumb of %q: %q", b.s.listing.Path, b.tt.Texts())
		}
		// The more menu holds the same groups once the sidebar gives way.
		b.tt.SetSize(760, 560)
		b.settle()
		b.press(en("more"))
		b.item(en("directory_devices"))

		// A remote Linux host: no Desktop or Applications, an unnamed root.
		b = newBench(t, Options{Mode: Directory, Source: NewCatalog(true).Source(), Host: "build-box", Remote: true})
		want = []string{"place " + en("directory_home"), "place " + en("directory_documents"), "place " + en("directory_downloads"),
			"device " + en("directory_root"), "device data"}
		if got := listed(); !slices.Equal(got, want) {
			t.Fatalf("the remote sidebar lists %q, want %q", got, want)
		}
		// A host that reports its home alone, as a Node before the places:
		// the one place, and no group of devices.
		bare := Source{Browse: func(directory string, done func(Listing, string)) {
			l, problem := catalog.List(home)
			l.Locations = []Location{{Kind: "home", Path: home}}
			done(l, problem)
		}}
		b = newBench(t, Options{Mode: Directory, Source: bare})
		if got := listed(); !slices.Equal(got, []string{"place " + en("directory_home")}) {
			t.Fatalf("a host without places lists %q", got)
		}
		b.lacks(en("directory_devices"))
		// Every kind and medium has its own icon.
		icons := map[*ui.SVG]string{}
		for _, loc := range []Location{{Kind: "home"}, {Kind: "desktop"}, {Kind: "documents"}, {Kind: "downloads"}, {Kind: "applications"},
			{Kind: "project"}, {Kind: "root", Medium: "internal"}, {Kind: "volume", Medium: "external"}, {Kind: "volume", Medium: "network"}} {
			if other, taken := icons[glyph(loc)]; taken || glyph(loc) == nil {
				t.Fatalf("%+v shares the icon of %s", loc, other)
			}
			icons[glyph(loc)] = loc.Kind + loc.Medium
		}
		if glyph(Location{Kind: "volume"}) != glyph(Location{Kind: "root", Medium: "internal"}) {
			t.Fatal("a volume of an unknown medium is no drive")
		}
	})

	// The demo shows the sample tree as a machine's own: no notice, and no
	// text, name or path that says the data is a sample.
	t.Run("demo", func(t *testing.T) {
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source(), Preview: true})
		b.has(en("directory_preview"))
		b.demo = true
		b.settle()
		b.lacks(en("directory_preview"))
		said := func() {
			t.Helper()
			for _, e := range b.tt.AccessAll() {
				for _, text := range []string{e.Name, e.Value, e.Description} {
					for _, word := range []string{"preview", "sample", "demo"} {
						if strings.Contains(strings.ToLower(text), word) {
							t.Fatalf("the demo shows %q", text)
						}
					}
				}
			}
		}
		said()
		for _, name := range []string{en("directory_documents"), "Backup", "Macintosh HD", en("directory_home")} {
			b.press(name)
			said()
		}
		b.double("sailry")
		said()
		b.press(en("directory_address"))
		said()
	})

	// The top band moves the window, so its empty parts take no click of
	// their own, and its controls keep theirs.
	t.Run("top bar", func(t *testing.T) {
		home := NewCatalog(false).Home()
		b := newBench(t, Options{Mode: Directory, Source: NewCatalog(false).Source()})
		b.click("scratch")
		title := b.text(en("directory_title"))
		grid, create := b.button(en("directory_grid")).Bounds, b.button(en("directory_create")).Bounds
		for _, at := range [][2]float32{{title.X + title.W + 30, title.Y + title.H/2}, {title.X + 4, title.Y - 10},
			{(grid.X + grid.W + create.X) / 2, grid.Y - 6}, {create.X + create.W + 2, create.Y + create.H/2}} {
			b.tt.ClickAt(at[0], at[1])
			b.tt.ClickAt(at[0], at[1])
			b.settle()
			if !b.s.Showing() || b.s.editing || b.s.listing.Path != home || b.s.current != "scratch" || b.s.Grid || b.s.naming.open {
				t.Fatalf("a click at %v of the top band did something", at)
			}
		}
		// The controls inside the band still take their clicks.
		b.press(en("directory_grid"))
		b.press(en("directory_address"))
		field, ok := b.tt.AccessByRole(ui.RoleTextField, en("directory_filter"))
		if !b.s.Grid || !b.s.editing || !ok {
			t.Fatalf("the band's controls lost their clicks: %v %v %v", b.s.Grid, b.s.editing, ok)
		}
		b.tt.ClickAt(field.Bounds.X+field.Bounds.W/2, field.Bounds.Y+field.Bounds.H/2)
		b.tt.Type("zz")
		b.settle()
		if b.s.filter != "zz" || b.s.editing {
			t.Fatalf("the filter took %q", b.s.filter)
		}
		b.press(en("directory_up"))
		if b.s.listing.Path != "/Users" {
			t.Fatalf("Up led to %q", b.s.listing.Path)
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
