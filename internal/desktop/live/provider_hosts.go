package live

import (
	"cmp"
	"runtime"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func (r *Runtime) providerHosts() []settings.ProviderHost {
	root := r.localRuntime()
	localNode := root.controller.Target().String()
	hosts := make([]settings.ProviderHost, 0, len(root.App.Data.Hosts)+1)
	seen := map[string]bool{}
	for _, host := range root.App.Data.Hosts {
		owner := root.runtimeFor(host.ID)
		hosts = append(hosts, settings.ProviderHost{
			Node: host.ID, Name: host.Name, Platform: host.Platform, Connected: owner != nil && owner.controller.Connected(), Local: host.ID == localNode,
		})
		seen[host.ID] = true
	}
	if !seen[localNode] {
		hosts = append(hosts, settings.ProviderHost{Node: localNode, Name: "local_host_name", Platform: runtime.GOOS, Connected: root.controller.Connected(), Local: true})
	}
	slices.SortFunc(hosts, func(a, b settings.ProviderHost) int {
		if a.Node == localNode {
			return -1
		}
		if b.Node == localNode {
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
