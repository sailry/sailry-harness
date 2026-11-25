package panel

import (
	"cmp"
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

func (s *State) SetResources(resources []Resource) {
	previous := s.ResourceList()
	s.resources = append([]Resource{}, resources...)
	slices.SortStableFunc(s.resources, func(a, b Resource) int { return cmp.Compare(a.Order, b.Order) })
	for _, tab := range s.Tabs {
		if s.ValidTab(tab) {
			continue
		}
		s.Tabs, s.Active = Closed(s.Tabs, tab, s.Active)
		if i := slices.IndexFunc(previous, func(r Resource) bool { return r.Kind == tab.Kind }); i >= 0 && previous[i].Release != nil {
			previous[i].Release(s, tab, TabDetached)
		}
	}
}

func (s *State) RegisteredResource(kind Kind) bool {
	_, exists := s.resource(kind)
	return exists
}

// ResourceList returns the shared ordered declarations for this panel.
func (s *State) ResourceList() []Resource { return s.resources }

func (s *State) resource(kind Kind) (Resource, bool) {
	for _, resource := range s.ResourceList() {
		if resource.Kind == kind {
			return resource, true
		}
	}
	return Resource{}, false
}

// Shortcut returns the effective chord for a resource. A resource with no
// declared shortcut can still be opened from the launcher and add menu.
func (s *State) Shortcut(resource Resource) (ui.Modifiers, ui.Key) {
	if resource.Key != 0 && s.ShortcutBinding != nil {
		return s.ShortcutBinding(resource)
	}
	return resource.Mods, resource.Key
}

// AddNumberedTab allocates a window-unique numeric tab ID, even for a tab
// opened in the background. An unregistered resource cannot allocate a tab.
func (s *State) AddNumberedTab(kind Kind, show bool) Tab {
	resource, ok := s.resource(kind)
	if !ok || !resource.Numbered {
		return Tab{}
	}
	s.pages++
	tab := Tab{Kind: kind, ID: strconv.Itoa(s.pages)}
	if show {
		s.Show(tab)
	} else {
		s.Tabs = Opened(s.Tabs, tab)
	}
	return tab
}

// Reserve keeps IDs unique across restored and parked tabs in the window.
func (s *State) Reserve(tabs ...Tab) {
	for _, tab := range tabs {
		if resource, ok := s.resource(tab.Kind); ok && resource.Numbered {
			if id, err := strconv.Atoi(tab.ID); err == nil {
				s.pages = max(s.pages, id)
			}
		}
	}
}

// CloseConfirmation describes an explicit close completed by the resource.
// Close starts that action; the host keeps the tab until it is removed.
type CloseConfirmation struct {
	Title string // i18n key
	Close func()
}

func (s *State) closeConfirmation(tab Tab) CloseConfirmation {
	if resource, ok := s.resource(tab.Kind); ok && resource.ConfirmClose != nil {
		return resource.ConfirmClose(s, tab)
	}
	return CloseConfirmation{}
}

func (s *State) confirmClose(c *ui.Context) {
	tab := s.closing
	if tab == (Tab{}) {
		return
	}
	if !slices.Contains(s.Tabs, tab) {
		s.closing = Tab{}
		return
	}
	request := s.closeConfirmation(tab)
	if request.Close == nil {
		s.closing = Tab{}
		return
	}
	open := true
	if kit.Confirm(c.Key(struct{ Close Tab }{tab}), &open, kit.Confirmation{
		Title: i18n.T(c, request.Title), Message: s.Title(c, tab),
		Buttons: []string{i18n.T(c, "settings_cancel"), i18n.T(c, "close")}, Default: 1, Destructive: []int{1},
	}) == 1 {
		s.closing = Tab{}
		if s.OnCloseConfirmed != nil {
			s.OnCloseConfirmed(tab)
		}
		request.Close()
	}
	if !open {
		s.closing = Tab{}
	}
}

// TabRelease distinguishes discarding a resource from detaching its strip
// without necessarily ending the underlying resource.
type TabRelease uint8

const (
	TabRemoved TabRelease = iota
	TabDetached
)

func (s *State) releaseTab(tab Tab, reason TabRelease) {
	if resource, ok := s.resource(tab.Kind); ok && resource.Release != nil {
		resource.Release(s, tab, reason)
	}
}

// DiscardTabChrome releases a permanently closed tab, not one parked in
// another conversation or moved between surfaces.
func (s *State) DiscardTabChrome(tab Tab) { s.releaseTab(tab, TabRemoved) }

// DetachTab releases a discarded panel surface while allowing its owner to
// keep any longer-lived state.
func (s *State) DetachTab(tab Tab) { s.releaseTab(tab, TabDetached) }

// Find delegates the native Find command to the active tab's renderer.
func (s *State) Find() bool {
	resource, ok := s.resource(s.Active.Kind)
	return ok && resource.Find != nil && resource.Find(s, s.Active)
}

// ValidTab also accepts kinds contributed by the current product, and rejects
// tabs saved for optional resources the product does not mount.
func (s *State) ValidTab(tab Tab) bool {
	switch tab.Kind {
	case History, Documents, Artifact, Subagent:
		return true
	case Files:
		if tab.ID != "" {
			return true // Explicit document tabs do not require the directory browser.
		}
	}
	_, exists := s.resource(tab.Kind)
	return exists
}

// RenderProjectResource uses the existing shared file or diff tab body.
func RenderProjectResource(c *ui.Context, s *State, tab Tab) {
	s.pr.view(c, tab.Kind, tab.ID, s.owns(tab), func(path string) { s.Show(Tab{tab.Kind, path}) },
		func(path string) { s.Show(Tab{Files, path}) })
}

// RenderFilePanelResource pairs the shared editor with the product's tree.
func RenderFilePanelResource(c *ui.Context, s *State, tab Tab, tree func(*Project, *ui.Context, bool, func(string))) {
	s.pr.fileView(c, tab.ID, s.owns(tab), func(path string) { s.Show(Tab{Kind: Files, ID: path}) }, tree)
}
