package shell

import (
	"cmp"
	"fmt"
	"slices"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
)

type panelRegistration struct {
	resource panel.Resource
	bind     []func(PanelResourceHost)
}

// RegisterPanelResource contributes one resource to the conversation panel.
// Binding runs once in a live window after its controllers are available.
func (a *Assembly) RegisterPanelResource(resource panel.Resource, bind ...func(PanelResourceHost)) {
	if resource.Kind == "" || resource.Label == "" || resource.Icon == nil || resource.Render == nil {
		panic(fmt.Sprintf("shell: panel resource %q requires a kind, label, icon, and renderer", resource.Kind))
	}
	if a.HasPanelResource(resource.Kind) {
		panic(fmt.Sprintf("shell: duplicate panel resource %q", resource.Kind))
	}
	a.panelResources = append(a.panelResources, panelRegistration{resource, slices.Clone(bind)})
}

func (a Assembly) HasPanelResource(kind panel.Kind) bool {
	return slices.ContainsFunc(a.panelResources, func(entry panelRegistration) bool { return entry.resource.Kind == kind })
}

type PanelResourceHost struct {
	App         *App
	BindBrowser func()
	// BindTerminal connects the shared live terminal service to this window.
	BindTerminal func() sidebar.ProjectOpen
}

func (a *App) installPanelResources(assembly Assembly) {
	resources := make([]panel.Resource, 0, len(assembly.panelResources))
	for _, registration := range assembly.panelResources {
		resources = append(resources, registration.resource)
	}
	a.Panel.SetResources(resources)
}

func (a *App) BindPanelResources(assembly Assembly, host PanelResourceHost) {
	host.App = a
	for _, registration := range assembly.panelResources {
		for _, bind := range registration.bind {
			bind(host)
		}
	}
}

// HasWorkspaceResource covers resources that share a project with the panel
// or with a central page.
func (a *App) HasWorkspaceResource(kind panel.Kind) bool {
	return a != nil && (a.Panel != nil && a.Panel.RegisteredResource(kind) || slices.ContainsFunc(a.nativePages, func(page NativePage) bool {
		return page.Resource == kind
	}))
}

func (a *App) RegisterProjectResourceAction(action sidebar.ProjectResourceAction) {
	if actions := a.Sidebar.Actions; actions != nil && action.Open != nil {
		actions.ProjectResources = append(actions.ProjectResources, action)
		slices.SortStableFunc(actions.ProjectResources, func(a, b sidebar.ProjectResourceAction) int { return cmp.Compare(a.Order, b.Order) })
	}
}
