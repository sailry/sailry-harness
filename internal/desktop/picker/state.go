package picker

import (
	"math"
	"slices"
	"strings"

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
	// "" for none.
	Host string
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
	goTo
	history
	refresh
)

type notice struct {
	outcome kit.Outcome
	key     string
	host    bool
}

// State is the one picker of a window. Grid and Hidden are the view
// preferences, kept across uses; Changed reports that the user set one.
type State struct {
	Grid, Hidden bool
	Changed      func()

	open       bool
	o          Options
	generation int
	busy       bool
	failed     bool
	loaded     bool
	listing    Listing
	history    []string
	cursor     int

	filter  string
	address string
	editing bool
	refocus bool // the entries take the keyboard focus in the next frame
	crumbs  ui.ScrollState
	current string // the selected entry's name, "" for none
	// chosen are the names of the entries chosen in Files mode, which takes
	// several: Cmd and Shift add to the click's, as in any list.
	chosen ui.Selection[string]
	row    int
	rows   []int
	list   ui.ListState
	grid   ui.GridState
	sort   ui.SortOrder
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
	s.navigate(o.Initial, push, 0)
}

// Showing reports whether the picker is open.
func (s *State) Showing() bool { return s.open }

// Close dismisses the picker and drops what it was reading.
func (s *State) Close() {
	s.generation++
	s.open, s.busy = false, false
	s.naming.open, s.trashing.open, s.jump.open = false, false, false
}

func (s *State) valid() bool { return s.o.Valid == nil || s.o.Valid() }

func (s *State) pending() bool { return s.o.Pending != nil && s.o.Pending() }

// navigate reads path and shows it when it arrives. A read that fails keeps
// the directory shown and blocks Confirm until another succeeds.
func (s *State) navigate(path string, how visit, cursor int) {
	if !s.open || s.busy || !s.valid() {
		return
	}
	s.generation++
	generation := s.generation
	s.busy = true
	s.o.Source.Browse(path, func(l Listing, problem string) {
		if !s.open || s.generation != generation || !s.valid() {
			return
		}
		s.busy = false
		if problem != "" {
			s.failed = true
			s.notices = append(s.notices, notice{kit.Failed, problem, false})
			return
		}
		s.arrive(l, how, cursor)
	})
}

func (s *State) arrive(l Listing, how visit, cursor int) {
	switch how {
	case push, goTo:
		if s.cursor >= len(s.history) || s.history[s.cursor] != l.Path {
			s.history = append(s.history[:min(s.cursor+1, len(s.history))], l.Path)
			s.cursor = len(s.history) - 1
		}
	case history:
		s.cursor = cursor
	}
	changed := !s.loaded || s.listing.Path != l.Path
	s.listing, s.loaded, s.failed = l, true, false
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

func (s *State) reload() {
	if s.loaded {
		s.navigate(s.listing.Path, refresh, 0)
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
	if paths == nil || s.busy || s.failed || s.pending() || !s.valid() {
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
		n := notice{kit.Done, "task_completed", true}
		switch problem {
		case "":
		case "directory_outcome_unknown":
			n = notice{kit.Uncertain, problem, true}
		case "files_paste_conflict":
			n = notice{kit.Conflict, problem, true}
		default:
			n = notice{kit.Failed, problem, true}
		}
		s.notices = append(s.notices, n)
		if a.Kind == "transfer" && a.Cut && problem == "" {
			s.clipboard = nil
		}
		s.reload()
	})
}

func (s *State) copy(cut bool) {
	if s.current != "" {
		s.clipboard, s.cut = []string{s.listing.Join(s.current)}, cut
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
		s.notices = append(s.notices, notice{kit.Failed, "directory_invalid_name", false})
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
			s.notices = append(s.notices, notice{kit.Failed, problem, false})
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
