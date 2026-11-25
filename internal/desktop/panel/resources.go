package panel

import (
	"cmp"
	"slices"

	"github.com/egoist/mygo/ui"
)

func (s *State) SetResources(resources []Resource) {
	s.resources = append([]Resource{}, resources...)
	slices.SortStableFunc(s.resources, func(a, b Resource) int { return cmp.Compare(a.Order, b.Order) })
}

func (s *State) RegisteredResource(kind Kind) bool {
	_, exists := s.resource(kind)
	return s.resources != nil && exists
}

// ResourceList returns the shared ordered declarations for this panel.
func (s *State) ResourceList() []Resource {
	if s.resources != nil {
		return s.resources
	}
	return Resources
}

func (s *State) resource(kind Kind) (Resource, bool) {
	for _, resource := range s.ResourceList() {
		if resource.Kind == kind {
			return resource, true
		}
	}
	return Resource{}, false
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
