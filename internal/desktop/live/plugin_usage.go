package live

import (
	"context"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
)

func (r *Runtime) pluginUsageSources() []plugins.UsageSource {
	root := r.localRuntime()
	sources := []plugins.UsageSource{{Client: root.controller, Label: root.controller.Target().String()}}
	if root.network == nil {
		return sources
	}
	for _, peer := range root.network.snapshot() {
		if peer.controller != nil {
			label := peer.address.ID.String()
			if peer.info.Name != nil && *peer.info.Name != "" {
				label = *peer.info.Name
			}
			sources = append(sources, plugins.UsageSource{Client: peer.controller, Label: label})
		}
	}
	return sources
}

func (r *Runtime) pluginUsageChanges(ctx context.Context) <-chan struct{} {
	root := r.localRuntime()
	changed := make(chan struct{}, 1)
	signal := func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	}
	var releases []func()
	if root.network != nil {
		releases = append(releases, root.network.onChange(signal))
	}
	retained := map[*client.Client]bool{}
	for _, source := range r.pluginUsageSources() {
		if source.Client != nil && !retained[source.Client] {
			retained[source.Client] = true
			releases = append(releases, source.Client.OnChange(signal))
		}
	}
	context.AfterFunc(ctx, func() {
		for _, release := range releases {
			release()
		}
	})
	return changed
}
