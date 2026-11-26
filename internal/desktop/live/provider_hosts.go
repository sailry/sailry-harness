package live

import (
	"cmp"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func (r *Runtime) providerHosts() []settings.ProviderHost {
	root := r.localRuntime()
	canonical := root.canonicalHosts()
	hosts := make([]settings.ProviderHost, 0, len(canonical))
	for _, entry := range canonical {
		hosts = append(hosts, settings.ProviderHost{
			Node: entry.IDString, Name: entry.NameKey, Platform: entry.Platform, Connected: entry.Connected, Local: entry.Local,
		})
	}
	slices.SortFunc(hosts, func(a, b settings.ProviderHost) int {
		if a.Local != b.Local {
			if a.Local {
				return -1
			}
			return 1
		}
		return strings.Compare(a.Name, b.Name)
	})
	return hosts
}

func (r *Runtime) providerDefaultTarget() string {
	root := r.localRuntime()
	if root.App.Page == shell.SettingsPage && root.App.Settings.Section == settings.Providers {
		return cmp.Or(root.App.Settings.ManagedNode, root.controller.Target().String())
	}
	if selected := root.selectedRuntime(); selected != nil {
		return selected.controller.Target().String()
	}
	return root.controller.Target().String()
}

func (r *Runtime) providerOwner(node string) *Runtime {
	root := r.localRuntime()
	owner := root.runtimeFor(node)
	if owner == nil || !owner.controller.Connected() {
		return nil
	}
	return owner
}
