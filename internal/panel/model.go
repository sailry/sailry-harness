// Package panel is the right panel beside the conversation: a row of
// resource tabs (files, changes, checkpoints, documents, browser,
// terminal) over the active one, or the launcher when none is open
// (sailry-public resources/*.rs, navigation_tabs.rs). The frame owns its
// width, motion and toggle (docs/shell.md); see docs/panel.md.
package panel

import (
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

// Opened adds k to the tabs, at the end, unless it is open already.
func Opened(tabs []Kind, k Kind) []Kind {
	if slices.Contains(tabs, k) {
		return tabs
	}
	return append(slices.Clone(tabs), k)
}

// Closed takes k out of the tabs and returns the tab chosen after: the
// active one when another closed, else the first left, as the reference
// moves the choice to the first open tab; "" when none is left.
func Closed(tabs []Kind, k, active Kind) ([]Kind, Kind) {
	out := slices.DeleteFunc(slices.Clone(tabs), func(t Kind) bool { return t == k })
	switch {
	case len(out) == 0:
		return out, ""
	case active != k && slices.Contains(out, active):
		return out, active
	}
	return out, out[0]
}

// State is the panel's own UI state over the preview workspace: its tabs
// and the resources no project keeps. Files and Git keep theirs in the
// shown Project, shared with their pages.
type State struct {
	Data   *preview.Workspace
	Tabs   []Kind
	Active Kind
	// Paged is the resource a page of the same project shows, "" for
	// none: that page owns the resource's scrolling (Page).
	Paged Kind

	doc     string
	address string
}

func New(w *preview.Workspace) *State {
	s := &State{Data: w}
	if len(w.Documents) > 0 {
		s.doc = w.Documents[0].ID
	}
	return s
}

// Open shows k, opening its tab if needed.
func (s *State) Open(k Kind) {
	s.Tabs, s.Active = Opened(s.Tabs, k), k
}

// Close closes k's tab. It reports whether a tab is left.
func (s *State) Close(k Kind) bool {
	s.Tabs, s.Active = Closed(s.Tabs, k, s.Active)
	return len(s.Tabs) > 0
}
