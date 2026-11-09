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
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
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

// Tab is an open item: a kind and which one of it. A file and a diff are
// by their path, each its own tab (ID "" until the shown project names
// the item to open); History has one tab (ID ""); a document is by its
// ID, a browser page and a terminal by a number.
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
	// pr is the shown project, whose open files and diffs are the
	// strip's file and diff tabs (sync); synced is set after the first
	// project, which takes the tabs restored at start-up.
	pr     *Project
	synced bool

	// Cwd is the folder a new terminal starts in, the shown project's
	// (the shell sets it).
	Cwd       string
	pages     int                            // browser pages and terminals opened, for their IDs
	artifacts map[string]conversation.File   // the conversation files previewed, by ID
	view      kit.Viewer[conversation.Image] // the image viewer over a previewed picture
	children  map[string]string              // the subagent sessions open, their titles by ID
	pagesOf   map[string]*page               // the browser tabs' addresses and history, by ID
	terms     map[string]*term               // the terminal tabs' state, by ID
	// Child builds a subagent's session in its tab, the transcript the
	// shell owns (subagents/panel/view.rs).
	Child func(c *ui.Context, id string)
}

func New(w *preview.Workspace) *State {
	return &State{Data: w}
}

// Open shows a resource of the launcher, the "+" menu or a shortcut: its
// tab when one is open, else a new one. Documents opens the first; Files
// and Changes the project's file or diff shown last.
func (s *State) Open(k Kind) {
	if item(k) {
		// The project's item shown last; the panel names it as it syncs
		// when no project is known yet.
		s.Show(Tab{Kind: k, ID: s.shown(k)})
		return
	}
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
	if !t.valid() {
		return
	}
	if item(t.Kind) && t.ID != "" && s.pr != nil {
		s.pr.show(t.Kind, t.ID)
	}
	s.Tabs, s.Active = Opened(s.Tabs, t), t
	if t.Kind == Browser || t.Kind == Terminal {
		var n int
		if _, err := fmt.Sscan(t.ID, &n); err == nil {
			s.pages = max(s.pages, n)
		}
	}
}

// valid reports whether the panel shows t's kind: a tab saved by an
// older build may name one it no longer opens.
func (t Tab) valid() bool {
	switch t.Kind {
	case Artifact, Subagent:
		return true
	}
	for _, r := range Resources {
		if r.Kind == t.Kind {
			return true
		}
	}
	return false
}

// Close closes t's tab. It reports whether a tab is left.
func (s *State) Close(t Tab) bool {
	if item(t.Kind) && t.ID != "" && s.pr != nil {
		s.pr.close(t.Kind, t.ID)
	}
	s.Tabs, s.Active = Closed(s.Tabs, t, s.Active)
	return len(s.Tabs) > 0
}

// item reports whether k's tabs are a project's files or diffs.
func item(k Kind) bool { return k == Files || k == Changes }

// shown is the shown project's item of k shown last, "" without one.
func (s *State) shown(k Kind) string {
	if s.pr == nil {
		return ""
	}
	return s.pr.shown(k)
}

// Sync makes the strip's file and diff tabs the project's open items, as
// the page's strip shows them (docs/panel.md, Components and hosts): a
// tab with no item yet takes the one shown last, items opened or closed
// on the page are added or dropped, and another project's are swapped
// for this one's. The first project takes the tabs restored at start-up.
// Without a project the tabs stay and show the project prompt.
func (s *State) Sync(pr *Project) {
	s.pr = pr
	if pr == nil {
		return
	}
	if !s.synced {
		s.synced = true
		for _, t := range s.Tabs {
			if item(t.Kind) && t.ID != "" {
				pr.show(t.Kind, t.ID)
			}
		}
		if item(s.Active.Kind) && s.Active.ID != "" {
			pr.show(s.Active.Kind, s.Active.ID)
		}
	}
	var tabs []Tab
	for _, t := range s.Tabs {
		if item(t.Kind) {
			if t.ID == "" && pr.shown(t.Kind) != "" {
				id := pr.shown(t.Kind)
				pr.show(t.Kind, id)
				if s.Active == t {
					s.Active = Tab{t.Kind, id}
				}
				t.ID = id
			}
			if t.ID != "" && !slices.Contains(pr.items(t.Kind), t.ID) {
				continue
			}
		}
		tabs = Opened(tabs, t)
	}
	for _, k := range []Kind{Files, Changes} {
		for _, id := range pr.items(k) {
			tabs = Opened(tabs, Tab{k, id})
		}
	}
	if !slices.Equal(tabs, s.Tabs) {
		if !slices.Contains(tabs, s.Active) {
			_, s.Active = Closed(s.Tabs, s.Active, s.Active)
			if !slices.Contains(tabs, s.Active) && len(tabs) > 0 {
				s.Active = tabs[0]
			}
		}
		s.Tabs = tabs
	}
}

// owns reports whether the panel owns the scrolling of t's resource,
// which a page of the same project showing it takes over.
func (s *State) owns(t Tab) bool {
	switch t.Kind {
	case Files:
		return s.Paged != Files
	case Changes:
		return s.Paged != Changes
	}
	return true
}
