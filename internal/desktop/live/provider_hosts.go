package live

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func (r *Runtime) providerHosts() []settings.ProviderHost {
	root := r.localRuntime()
	localNode := root.controller.Target().String()
	hosts := make([]settings.ProviderHost, 0, len(root.App.Data.Hosts)+1)
	seen := map[string]bool{}
	for _, host := range root.App.Data.Hosts {
		owner := root.runtimeFor(host.ID)
		hosts = append(hosts, settings.ProviderHost{
			Node: host.ID, Name: host.Name, Connected: owner != nil && owner.controller.Connected(),
		})
		seen[host.ID] = true
	}
	if !seen[localNode] {
		hosts = append(hosts, settings.ProviderHost{Node: localNode, Name: "local_host_name", Connected: root.controller.Connected()})
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
