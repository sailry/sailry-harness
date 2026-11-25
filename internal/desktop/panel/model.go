// Package panel is the right panel beside the conversation: a row of resource
// tabs (files, changes, checkpoints, documents, browser, terminal) over the
// active one, or the launcher when none is open. The frame owns its width,
// motion and toggle (internal/desktop/shell).
package panel

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
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

// Resource is a kind's tab: its icon and label, and the shortcut that opens it, if any.
type Resource struct {
	Kind  Kind
	Icon  *ui.SVG
	Label string // i18n key
	Order int
	Mods  ui.Modifiers
	Key   ui.Key
	// Keystroke accepts a single manifest-style chord when Key is not set.
	Keystroke string
	Available func(*State) bool
	// Open overrides the standard open-or-focus action; New opens a fresh instance.
	Open         func(*State)
	New          func(*State)
	Render       func(*ui.Context, *State, Tab)
	Title        func(*State, Tab) string
	Busy         func(*State, Tab) bool
	TabMenu      func(*ui.Context, *ui.Menu, *State, Tab)
	Find         func(*State, Tab) bool
	ConfirmClose func(*State, Tab) CloseConfirmation
	Release      func(*State, Tab, TabRelease)
	// Numbered reserves restored numeric tab IDs for AddNumberedTab.
	Numbered      bool
	BindWorkspace GitWorkspaceMount
}

// Resources is the standalone sample catalog and metadata for built-in tabs.
// Product panels only offer their installed resource declarations.
var Resources = []Resource{
	{Kind: Browser, Icon: icons.Web, Label: "resource_browser", Order: 10, Mods: ui.Cmd, Key: ui.KeyT},
	{Kind: Terminal, Icon: icons.Terminal, Label: "terminal", Order: 20, Mods: ui.Ctrl, Key: ui.KeyBackquote},
	{Kind: Files, Icon: icons.Folder, Label: "files", Order: 30, Mods: ui.Cmd, Key: ui.KeyP},
	{Kind: Changes, Icon: icons.Network, Label: "git", Order: 40, Mods: ui.Ctrl | ui.Shift, Key: ui.KeyG},
	{Kind: History, Icon: icons.Restore, Label: "git_view_history"},
	{Kind: Documents, Icon: icons.FileText, Label: "market_category_documents"},
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
// all.
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
	resources       []Resource
	createResource  func(Kind) bool
	ShortcutBinding func(Resource) (ui.Modifiers, ui.Key)
	Actions         *Actions
	TerminalActions *TerminalActions
	BrowserActions  *BrowserActions
	documentView    func(*ui.Context, string) bool
	historyConfirm  confirm
	Live            bool
	// Demo is the demo conversations' panel: its terminal shows the demo's
	// screen and its browser the demo's page, as no shell starts and nothing
	// is loaded.
	Demo       bool
	HasProject bool
	// Session reports that the live conversation shown has a session: an
	// unsent draft has no checkpoints or documents to list (lists).
	Session bool
	// Worktree reports that the live conversation shown has a worktree on a
	// connected Node, where a terminal can start: an unsent chat without a
	// project has none, so Terminal is not offered there (lists).
	Worktree      bool
	closing       Tab // tab waiting for confirmation
	closingEditor *editorCloseRequest
	Data          *preview.Workspace
	Tabs          []Tab
	Active        Tab
	opened        []Kind // kinds of the tabs opened since TakeOpened
	// Paged is the resource a page of the same project shows, "" for
	// none: that page owns the resource's scrolling (Page).
	Paged Kind
	// The workspace keeps the editors, but the strip is conversation-scoped.
	// A new mount or an explicit clear excludes the project's existing tabs
	// from automatic import; new project items can still appear in this scope.
	pr, bound *Project // available now, and last bound in this mount
	isolated  bool
	ignored   []Tab

	// Cwd is the folder a new terminal starts in, the shown project's
	// (the shell sets it).
	Cwd               string
	pages             int                          // browser pages and terminals opened, for their IDs
	artifacts         map[string]conversation.File // the conversation files previewed, by ID
	artifactCallbacks ArtifactCallbacks
	view              fileview.View     // the previewed file's own state
	children          map[string]string // the subagent sessions open, their titles by ID
	pagesOf           map[string]*page  // the browser tabs' addresses and history, by ID
	focusAddress      string            // browser tab waiting to focus its address
	terms             map[string]*term  // the terminal tabs' state, by ID
	// Child builds a subagent's session in its tab, the transcript the shell
	// owns.
	Child func(c *ui.Context, id string)
	// Focus puts the keyboard focus in the pane that hosts the panel, as
	// the host sets it.
	Focus func(c *ui.Context)
	// OnEmpty is called when an explicit tab close removes the last tab.
	OnEmpty func()
	// OnCloseConfirmed records the origin of an explicit asynchronous close.
	OnCloseConfirmed func(Tab)
	// Deprecated: use OnCloseConfirmed. Retained for RegisterTerminal callers.
	OnTerminalClose func(Tab)
	// SharedItem reports whether another panel still uses a project item.
	SharedItem func(*Project, Tab) bool
}

func New(w *preview.Workspace) *State {
	return &State{Data: w, resources: []Resource{}}
}

// NewSample explicitly installs isolated resources for a standalone sample.
func NewSample(w *preview.Workspace) *State {
	state := New(w)
	resources := slices.Clone(Resources)
	for i := range resources {
		r := &resources[i]
		switch r.Kind {
		case Browser:
			r.Numbered, r.New, r.Render = true, (*State).OpenBrowserTab, RenderBrowserSample
			r.Title, r.Busy, r.Release = BrowserTitle, BrowserBusy, ReleaseBrowserTab
		case Terminal:
			r.Available = func(s *State) bool { return !s.Live || s.Worktree }
			r.Numbered, r.Open, r.Render = true, OpenTerminalTab, RenderTerminalTab
			r.ConfirmClose, r.Release, r.Find = fixtureTerminalClose, ReleaseTerminalTab, FindTerminalTab
		case Files, Changes:
			r.Render = RenderProjectResource
		}
	}
	state.SetResources(resources)
	return state
}

// Open shows a resource from the launcher, the "+" menu or a shortcut.
// By default it focuses an existing tab or opens the first one.
func (s *State) Open(k Kind) {
	resource, exists := s.resource(k)
	if exists {
		if resource.Available != nil && !resource.Available(s) {
			return
		}
		if resource.Open != nil {
			resource.Open(s)
			return
		}
	}
	if !s.ValidTab(Tab{Kind: k}) {
		return
	}
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
		// Without a document the tab shows the empty state, and takes the
		// first document once there is one (nameDocument).
		t := Tab{Kind: Documents}
		if len(s.Data.Documents) > 0 {
			t.ID = s.Data.Documents[0].ID
		}
		s.Show(t)
	default:
		if resource.New != nil {
			resource.New(s)
			return
		}
		s.Show(Tab{Kind: k})
	}
}

// New explicitly requests another tab, even if the resource's launcher entry
// is unavailable. An installed resource can report why it cannot open.
func (s *State) New(k Kind) {
	if resource, exists := s.resource(k); exists {
		if resource.New != nil {
			resource.New(s)
		} else if resource.Open != nil {
			resource.Open(s)
		}
	}
}

// Show shows t, opening its tab if needed.
func (s *State) Show(t Tab) {
	if !s.ValidTab(t) {
		return
	}
	// A file or diff opens in the passing tab, in the place of the one
	// that passed before.
	if item(t.Kind) && t.ID != "" && s.pr != nil {
		old := Tab{t.Kind, *s.pr.passingOf(t.Kind)}
		s.pr.show(t.Kind, t.ID)
		if i := slices.Index(s.Tabs, old); old.ID != "" && i >= 0 && !slices.Contains(s.pr.items(t.Kind), old.ID) && !slices.Contains(s.Tabs, t) {
			s.Tabs = slices.Clone(s.Tabs)
			s.Tabs[i] = t
		}
	}
	if !slices.Contains(s.Tabs, t) {
		s.opened = append(s.opened, t.Kind)
	}
	s.Tabs, s.Active = Opened(s.Tabs, t), t
	s.Reserve(t)
}

// passes reports whether t's tab only passes: a file's or a diff's, until
// kept. A terminal, a browser page and the others are opened on purpose and
// kept.
func (s *State) passes(t Tab) bool {
	return item(t.Kind) && s.pr != nil && s.pr.passes(t.Kind, t.ID)
}

// keeper is t's keep and release, nil for a tab that is always kept.
func (s *State) keeper(t Tab) (keep, release func()) {
	if !item(t.Kind) || s.pr == nil || t.ID == "" {
		return nil, nil
	}
	pr := s.pr
	return func() { pr.keep(t.Kind, t.ID) }, func() { pr.release(t.Kind, t.ID) }
}

// Mount replaces the strip without closing the outgoing conversation's resources.
func (s *State) Mount(tabs []Tab, active Tab) {
	s.cancelClose()
	s.keepProjectTabs()
	s.pr, s.bound, s.ignored = nil, nil, nil
	s.isolated = true
	s.Tabs = nil
	for _, t := range tabs {
		if s.ValidTab(t) {
			s.Tabs = Opened(s.Tabs, t)
		}
	}
	s.Reserve(s.Tabs...)
	s.Active = Tab{}
	if slices.Contains(s.Tabs, active) {
		s.Active = active
	} else if len(s.Tabs) > 0 {
		s.Active = s.Tabs[len(s.Tabs)-1]
	}
}

// Clear closes the strip, not the resources shared with the workspace. File
// drafts stay in the project's editors, and terminal shells stay on the Node.
func (s *State) Clear() {
	s.cancelClose()
	s.keepProjectTabs()
	for _, t := range s.Tabs {
		s.DetachTab(t)
	}
	s.isolated, s.ignored = true, projectTabs(s.bound)
	s.Tabs, s.Active = nil, Tab{}
}

func (s *State) keepProjectTabs() {
	for _, t := range s.Tabs {
		s.keepProjectTab(t)
	}
}

func (s *State) keepProjectTab(t Tab) {
	pr := s.bound
	if pr == nil || !item(t.Kind) || t.ID == "" {
		return
	}
	if t.Kind == Files {
		if editor := pr.Files.items[t.ID]; editor != nil {
			pr.syncDocument(t.ID, editor)
		}
	}
	pr.keep(t.Kind, t.ID)
}

func (s *State) cancelClose() {
	s.closing = Tab{}
	s.cancelEditorClose()
}

func (s *State) cancelEditorClose() {
	if s.closingEditor != nil && s.pr != nil && s.pr.closing == s.closingEditor {
		s.pr.closing = nil
	}
	s.closingEditor = nil
}

// Valid reports whether the panel shows t's kind: a tab saved by an
// older build may name one it no longer opens.
func (t Tab) Valid() bool {
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

// Close closes t's tab, or lets its resource confirm an asynchronous close.
// It reports whether a tab is left.
func (s *State) Close(t Tab) bool {
	if !slices.Contains(s.Tabs, t) {
		return len(s.Tabs) > 0
	}
	if s.closeConfirmation(t).Close != nil {
		s.closing = t
		return true
	}
	if pr := s.pr; item(t.Kind) && t.ID != "" {
		switch {
		case pr == nil || s.SharedItem != nil && s.SharedItem(pr, t):
			s.keepProjectTab(t)
			s.ignored = Opened(s.ignored, t)
		case t.Kind == Files:
			previous := pr.closing
			if pr.requestEditorClose(t.ID, func() {
				s.closingEditor = nil
				pr.Files.Close(t.ID)
				s.remove(t)
			}) {
				if previous == nil {
					s.closingEditor = pr.closing
				}
				return true
			}
			pr.Files.Close(t.ID)
		case t.Kind == Changes:
			pr.Git.Close(t.ID)
		}
	}
	s.remove(t)
	return len(s.Tabs) > 0
}

func (s *State) remove(t Tab) {
	if !slices.Contains(s.Tabs, t) {
		return
	}
	s.Tabs, s.Active = Closed(s.Tabs, t, s.Active)
	s.DiscardTabChrome(t)
	if len(s.Tabs) == 0 && s.OnEmpty != nil {
		s.OnEmpty()
	}
}

// lists reports whether the launcher and the "+" menu offer k. Checkpoints
// and session documents have their own entry points, not launcher entries.
func (s *State) lists(k Kind) bool {
	if k == History || k == Documents {
		return false
	}
	resource, exists := s.resource(k)
	return exists && (resource.Available == nil || resource.Available(s))
}

// Offers reports whether the conversation shown can open k: a shortcut opens
// only what the launcher lists.
func (s *State) Offers(k Kind) bool { return s.lists(k) }

// nameDocument gives the Documents tab opened without a document the first
// one, once the session has any.
func (s *State) nameDocument() {
	blank := Tab{Kind: Documents}
	i := slices.Index(s.Tabs, blank)
	if i < 0 || len(s.Data.Documents) == 0 {
		return
	}
	first := Tab{Documents, s.Data.Documents[0].ID}
	if slices.Contains(s.Tabs, first) {
		s.Tabs = slices.Delete(slices.Clone(s.Tabs), i, i+1)
	} else {
		s.Tabs = slices.Clone(s.Tabs)
		s.Tabs[i] = first
	}
	if s.Active == blank {
		s.Active = first
	}
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

// BindProject binds the current workspace before it or the panel modifies its
// items. A mounted strip brings its own tabs, not another conversation's.
func (s *State) BindProject(pr *Project) {
	if s.pr != pr {
		s.cancelEditorClose()
	}
	s.pr = pr
	if pr == nil || s.bound == pr {
		return
	}
	if s.isolated {
		s.ignored = projectTabs(pr)
	}
	if s.bound == nil {
		for _, t := range s.Tabs {
			if item(t.Kind) && t.ID != "" {
				pr.retain(t)
			}
		}
		if item(s.Active.Kind) && s.Active.ID != "" && slices.Contains(s.Tabs, s.Active) {
			pr.show(s.Active.Kind, s.Active.ID)
		}
	}
	s.bound = pr
}

func projectTabs(pr *Project) []Tab {
	var tabs []Tab
	if pr != nil {
		for _, k := range []Kind{Files, Changes} {
			for _, id := range pr.items(k) {
				tabs = append(tabs, Tab{k, id})
			}
		}
	}
	return tabs
}

// Sync reflects changes to project items in the mounted strip. Without a
// project the tabs stay and show the project prompt.
func (s *State) Sync(pr *Project) {
	s.BindProject(pr)
	if pr == nil {
		return
	}
	var tabs []Tab
	for _, t := range s.Tabs {
		if !s.ValidTab(t) {
			continue
		}
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
		if !s.ValidTab(Tab{Kind: k}) {
			continue
		}
		for _, id := range pr.items(k) {
			if t := (Tab{k, id}); !slices.Contains(s.ignored, t) {
				tabs = Opened(tabs, t)
			}
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
