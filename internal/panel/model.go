// Package panel is the right panel beside the conversation: a row of
// resource tabs (files, changes, checkpoints, documents, browser,
// terminal) over the active one, or the launcher when none is open
// (sailry-public resources/*.rs, navigation_tabs.rs). The frame owns its
// width, motion and toggle (docs/shell.md); see docs/panel.md.
package panel

import (
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/preview"
)

// Kind is a resource the panel shows in a tab.
type Kind string

const (
	Files     Kind = "files"
	Changes   Kind = "git"
	History   Kind = "checkpoints"
	Documents Kind = "documents"
	Browser   Kind = "browser"
	Terminal  Kind = "terminal"
)

// Resource is a kind's tab: its icon and label, and the shortcut that
// opens it (plugins/catalog.json renderer shortcuts), if any.
type Resource struct {
	Kind  Kind
	Icon  *ui.SVG
	Label string // i18n key
	Mods  ui.Modifiers
	Key   ui.Key
}

// Resources are the kinds in the launcher's order: the reference's
// Browser, Terminal, Files and Review, then checkpoints and documents.
var Resources = []Resource{
	{Browser, icons.Web, "resource_browser", ui.Cmd, ui.KeyT},
	{Terminal, icons.Terminal, "terminal", ui.Ctrl, ui.KeyBackquote},
	{Files, icons.Folder, "files", ui.Cmd, ui.KeyP},
	{Changes, icons.Network, "git", ui.Ctrl | ui.Shift, ui.KeyG},
	{History, icons.Restore, "git_view_history", 0, 0},
	{Documents, icons.FileText, "market_category_documents", 0, 0},
}

// ResourceOf returns a kind's resource.
func ResourceOf(k Kind) Resource {
	for _, r := range Resources {
		if r.Kind == k {
			return r
		}
	}
	return Resource{Kind: k}
}

// Overflow is how many tabs the strip shows before its chevron lists them
// all (navigation_tabs.rs: more than 4).
const Overflow = 4

// Item kinds that only tabs hold: a file of the tree and a change's diff.
const (
	File Kind = "file"
	Diff Kind = "diff"
)

// Tab is an open item: a kind and which one of it. Files, Changes and
// History have one tab each (ID ""); a file and a diff are by path, a
// document by its ID, a browser page and a terminal by a number.
type Tab struct {
	Kind Kind   `json:"kind"`
	ID   string `json:"id,omitempty"`
}

// Opened adds t to the tabs, at the end, unless it is open already.
func Opened(tabs []Tab, t Tab) []Tab {
	if slices.Contains(tabs, t) {
		return tabs
	}
	return append(slices.Clone(tabs), t)
}

// Closed takes t out of the tabs and returns the tab chosen after: the
// active one when another closed, else the first left, as the reference
// moves the choice to the first open tab; the zero Tab when none is left.
func Closed(tabs []Tab, t, active Tab) ([]Tab, Tab) {
	out := slices.DeleteFunc(slices.Clone(tabs), func(o Tab) bool { return o == t })
	switch {
	case len(out) == 0:
		return out, Tab{}
	case active != t && slices.Contains(out, active):
		return out, active
	}
	return out, out[0]
}

// State is the panel's own UI state over the preview workspace: one flat
// row of tabs, each an open item, and the resources no project keeps.
// Files and Git keep theirs in the shown Project, shared with their pages.
type State struct {
	Data   *preview.Workspace
	Tabs   []Tab
	Active Tab
	// Paged is the resource a page of the same project shows, "" for
	// none: that page owns the resource's scrolling (Page).
	Paged Kind

	address string
	pages   int // browser pages and terminals opened, for their IDs
}

func New(w *preview.Workspace) *State {
	return &State{Data: w}
}

// Open shows a resource of the launcher, the "+" menu or a shortcut: its
// tab when one is open, else a new one. Documents opens the first.
func (s *State) Open(k Kind) {
	for _, t := range s.Tabs {
		if t.Kind == k {
			s.Show(t)
			return
		}
	}
	switch k {
	case Documents:
		if len(s.Data.Documents) == 0 {
			return
		}
		s.Show(Tab{Documents, s.Data.Documents[0].ID})
	case Browser, Terminal:
		s.New(k)
	default:
		s.Show(Tab{Kind: k})
	}
}

// New opens another browser page or terminal.
func (s *State) New(k Kind) {
	s.pages++
	s.Show(Tab{k, fmt.Sprint(s.pages)})
}

// Show shows t, opening its tab if needed.
func (s *State) Show(t Tab) {
	s.Tabs, s.Active = Opened(s.Tabs, t), t
	if t.Kind == Browser || t.Kind == Terminal {
		var n int
		if _, err := fmt.Sscan(t.ID, &n); err == nil {
			s.pages = max(s.pages, n)
		}
	}
}

// Close closes t's tab. It reports whether a tab is left.
func (s *State) Close(t Tab) bool {
	s.Tabs, s.Active = Closed(s.Tabs, t, s.Active)
	return len(s.Tabs) > 0
}

// owns reports whether the panel owns the scrolling of t's resource,
// which a page of the same project showing it takes over.
func (s *State) owns(t Tab) bool {
	switch t.Kind {
	case Files, File:
		return s.Paged != Files
	case Changes, Diff:
		return s.Paged != Changes
	}
	return true
}
