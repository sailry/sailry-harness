package picker

import (
	"math"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// Options open the picker for one caller.
type Options struct {
	Mode Mode
	// Title and Confirm are i18n keys: the dialog's title and its primary
	// button.
	Title, Confirm string
	// Initial is the directory first shown, the host's home for "".
	Initial string
	Source  Source
	// Host names the execution host in the outcome toasts of file actions,
	// "" for none. Remote marks a host other than this device: the picker
	// then shows the name, after the heading of the sidebar's first group
	// or, without the sidebar, after the title.
	Host   string
	Remote bool
	// Projects are the directories of the projects registered on the host,
	// the most recently used first: a group of the sidebar between the
	// host's places and its devices, left out when empty. Only Name and Path
	// are read.
	Projects []Location
	// Preview marks the catalog of sample directories.
	Preview bool
	// Valid, when set, reports whether the caller still wants the choice: a
	// form whose host or path changed makes the picker stale.
	Valid func() bool
	// Pending, when set, reports whether the caller is still applying the
	// last choice of a picker it kept open.
	Pending func() bool
	// Choose takes the chosen paths, one in Directory and File mode, and
	// reports whether the picker closes.
	Choose func(paths []string) bool
}

type visit int

const (
	push visit = iota
	// first is the directory the picker opens at.
	first
	// fallback is the host's home, read in place of a first directory
	// that cannot show.
	fallback
	goTo
	history
	refresh
)

type notice struct {
	outcome kit.Outcome
	key     string
	host    bool
	path    string // what the text's %{path} names
}

// State is the one picker of a window. Grid and Hidden are the view
// preferences, kept across uses; Changed reports that the user set one.
type State struct {
	Grid, Hidden bool
	Changed      func()

	open bool
	o    Options
	// moved is how far the user dragged the dialog from the window's
	// centre, by its top band; a new opening starts at the centre.
	moved      [2]float32
	generation int
	busy       bool   // a file action runs, and the directory waits for its outcome
	reading    bool   // a directory is being read; navigating elsewhere replaces the read
	target     string // the directory being read, else the one shown or the one that failed to show
	problem    string // why target cannot show, an i18n key: set only while no directory shows
	missed     notice // the first directory and why it cannot show, while the home is read in its place
	loaded     bool
	listing    Listing
	history    []string
	cursor     int

	filter  string
	address string
	editing bool
	typing  bool // the address field has held the keyboard focus
	refocus bool // the entries take the keyboard focus in the next frame
	crumbs  ui.ScrollState
	current string // the selected entry's name, "" for none
	// chosen are the names of the entries chosen in Files mode, which takes
	// several: Cmd and Shift add to the click's, as in any list.
	chosen ui.Selection[string]
	row    int
	rows   []int
	// refused is the frame in which a click or a key landed on an entry
	// the mode cannot choose.
	refused time.Time
	list    ui.ListState
	grid    ui.GridState
	sort    ui.SortOrder
	// sorted is the order the entries were last arranged in: a column
	// header that changes it moves the list to the chosen entry, or to its
	// top.
	sorted ui.SortOrder
	view   int

	clipboard []string
	cut       bool
	naming    struct {
		open  bool
		from  string // the entry renamed, "" for a new folder
		value string
	}
	trashing struct {
		open  bool
		path  string
		label string
	}
	jump struct {
		open  bool
		query string
		row   int
	}
	children     map[string][]Crumb
	childrenBusy map[string]bool
	notices      []notice
	dismiss      string // the crumb whose menu closes: its directories failed to load
}

// Open shows the picker for o, replacing any earlier use.
func (s *State) Open(o Options) {
	s.generation++
	*s = State{Grid: s.Grid, Hidden: s.Hidden, Changed: s.Changed, generation: s.generation, open: true, o: o, row: -1,
		children: map[string][]Crumb{}, childrenBusy: map[string]bool{}}
	s.navigate(o.Initial, first, 0)
}

// Showing reports whether the picker is open.
func (s *State) Showing() bool { return s.open }

// Close dismisses the picker and drops what it was reading.
func (s *State) Close() {
	s.generation++
	s.open, s.busy, s.reading = false, false, false
	s.naming.open, s.trashing.open, s.jump.open = false, false, false
}

func (s *State) valid() bool { return s.o.Valid == nil || s.o.Valid() }

func (s *State) selectable(e Entry) bool { return Selectable(s.o.Mode, e) }

func (s *State) pending() bool { return s.o.Pending != nil && s.o.Pending() }

// navigate reads path and shows it when it arrives. A read blocks nothing:
// navigating elsewhere replaces it, and its answer is dropped. One that fails
// keeps the directory shown and says why in a toast. With no directory shown
// yet, the picker's first falls back to the host's home, and any other leaves
// its reason in the entries' place, with Retry.
func (s *State) navigate(path string, how visit, cursor int) {
	if !s.open || s.busy || !s.valid() {
		return
	}
	s.generation++
	generation := s.generation
	s.reading, s.target, s.problem = true, path, ""
	clear(s.childrenBusy)
	s.o.Source.Browse(path, func(l Listing, problem string) {
		if !s.open || s.generation != generation {
			return
		}
		s.reading = false
		switch {
		case !s.valid():
		case problem == "":
			s.arrive(l, how, cursor)
		case s.loaded:
			s.target = s.listing.Path
			s.notices = append(s.notices, notice{outcome: kit.Failed, key: problem})
		case how == first && path != "":
			s.missed = notice{key: problem, path: path}
			s.navigate("", fallback, 0)
		case how == fallback:
			// The home fails as well: the directory asked for keeps the
			// entries' place with its own reason, and Retry opens again.
			s.problem, s.target, s.address, s.refocus = s.missed.key, s.missed.path, s.missed.path, true
		default:
			s.problem, s.address, s.refocus = problem, path, true
		}
	})
}

func (s *State) arrive(l Listing, how visit, cursor int) {
	switch how {
	case fallback:
		// The sample tree holds no caller's path, so the preview says
		// nothing of it.
		if !s.o.Preview {
			s.notices = append(s.notices, notice{outcome: kit.Failed, key: "directory_fallback", path: s.missed.path})
		}
		fallthrough
	case push, first, goTo:
		if s.cursor >= len(s.history) || s.history[s.cursor] != l.Path {
			s.history = append(s.history[:min(s.cursor+1, len(s.history))], l.Path)
			s.cursor = len(s.history) - 1
		}
	case history:
		s.cursor = cursor
	}
	changed := !s.loaded || s.listing.Path != l.Path
	s.listing, s.loaded, s.target, s.problem = l, true, l.Path, ""
	s.address, s.editing, s.refocus = l.Path, false, true
	s.crumbs.X = math.MaxFloat32
	if changed {
		s.filter, s.current, s.row = "", "", -1
		s.chosen.Clear()
	} else if !slices.ContainsFunc(l.Entries, func(e Entry) bool { return e.Name == s.current }) {
		s.current, s.row = "", -1
	}
	if how == goTo {
		s.jump.open = false
	}
	clear(s.children)
	clear(s.childrenBusy)
}

func (s *State) step(forward bool) {
	cursor := s.cursor - 1
	if forward {
		cursor = s.cursor + 1
	}
	if cursor >= 0 && cursor < len(s.history) {
		s.navigate(s.history[cursor], history, cursor)
	}
}

func (s *State) up() {
	if s.loaded && s.listing.Parent != "" {
		s.navigate(s.listing.Parent, push, 0)
	}
}

// reload reads the directory shown again, or the one that failed to show.
func (s *State) reload() {
	if s.loaded {
		s.navigate(s.listing.Path, refresh, 0)
	} else if s.target == s.o.Initial {
		s.navigate(s.target, first, 0)
	} else {
		s.navigate(s.target, push, 0)
	}
}

// selected is the selected entries' indexes in the listing, as Resolve takes
// them: those chosen in Files mode, else the one selected.
func (s *State) selected() []int {
	if s.o.Mode == Files {
		var out []int
		for i, e := range s.listing.Entries {
			if s.chosen.Has(e.Name) {
				out = append(out, i)
			}
		}
		return out
	}
	if i := slices.IndexFunc(s.listing.Entries, func(e Entry) bool { return e.Name == s.current }); s.current != "" && i >= 0 {
		return []int{i}
	}
	return nil
}

// selection is what Confirm would choose.
func (s *State) selection() []string {
	if !s.loaded {
		return nil
	}
	return Resolve(s.o.Mode, s.listing, s.selected())
}

// activate opens a directory, or chooses a file alone in the file modes
// (double click and Enter).
func (s *State) activate(index int) {
	if index < 0 || index >= len(s.listing.Entries) {
		return
	}
	e := s.listing.Entries[index]
	switch {
	case e.Dir():
		s.navigate(s.listing.Join(e.Name), push, 0)
	case e.Kind == "file" && s.o.Mode != Directory:
		s.current = e.Name
		s.chosen.Clear()
		s.chosen.Add(e.Name)
		s.confirm()
	}
}

func (s *State) confirm() {
	paths := s.selection()
	if paths == nil || s.busy || s.reading || s.pending() || !s.valid() {
		return
	}
	if s.o.Choose == nil || s.o.Choose(paths) {
		s.Close()
	}
}

// manage applies a file action and reloads the directory; the toast names
// the host the files are on.
func (s *State) manage(a Action) {
	if s.busy || !s.valid() || s.o.Source.Manage == nil {
		return
	}
	generation := s.generation
	s.busy = true
	s.o.Source.Manage(a, func(problem string) {
		if !s.open || s.generation != generation {
			return
		}
		s.busy = false
		n := notice{outcome: kit.Done, key: "task_completed", host: true}
		switch problem {
		case "":
		case "directory_outcome_unknown":
			n = notice{outcome: kit.Uncertain, key: problem, host: true}
		case "files_paste_conflict":
			n = notice{outcome: kit.Conflict, key: problem, host: true}
		default:
			n = notice{outcome: kit.Failed, key: problem, host: true}
		}
		s.notices = append(s.notices, n)
		if a.Kind == "transfer" && a.Cut && problem == "" {
			s.clipboard = nil
		}
		s.reload()
	})
}

// copy puts the entry named name on the picker's clipboard, "" for none.
func (s *State) copy(name string, cut bool) {
	if name != "" {
		s.clipboard, s.cut = []string{s.listing.Join(name)}, cut
	}
}

func (s *State) paste(conflict string) {
	if len(s.clipboard) > 0 && s.loaded {
		s.manage(Action{Kind: "transfer", Paths: s.clipboard, Destination: s.listing.Path, Cut: s.cut, Conflict: conflict})
	}
}

// saveName creates the folder or renames the entry the name dialog is for.
func (s *State) saveName() {
	name := strings.TrimSpace(s.naming.value)
	if !ValidName(name) {
		s.notices = append(s.notices, notice{outcome: kit.Failed, key: "directory_invalid_name"})
		return
	}
	s.naming.open = false
	if s.naming.from == "" {
		s.manage(Action{Kind: "create", Path: s.listing.Join(name)})
	} else if name != s.naming.from {
		s.manage(Action{Kind: "rename", Path: s.listing.Join(s.naming.from), To: s.listing.Join(name)})
	}
}

// Create makes a folder named name in the directory shown, as the name
// dialog does.
func (s *State) Create(name string) {
	s.naming.from, s.naming.value = "", name
	s.saveName()
}

// loadChildren reads the directories under a crumb for its menu.
func (s *State) loadChildren(path string) {
	if s.childrenBusy[path] || !s.valid() {
		return
	}
	if _, ok := s.children[path]; ok {
		return
	}
	s.childrenBusy[path] = true
	generation := s.generation
	s.o.Source.Browse(path, func(l Listing, problem string) {
		if !s.open || s.generation != generation {
			return
		}
		delete(s.childrenBusy, path)
		if problem != "" {
			s.notices = append(s.notices, notice{outcome: kit.Failed, key: problem})
			s.dismiss = path
			return
		}
		list := []Crumb{}
		for _, e := range l.Entries {
			if e.Dir() && (s.Hidden || !strings.HasPrefix(e.Name, ".")) {
				list = append(list, Crumb{e.Name, l.Join(e.Name)})
			}
		}
		s.children[path] = list
	})
}
