package live

import (
	"context"
	"slices"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
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

// pluginUsageSample is the demo's usage while Demo mode shows, which the
// usage calls of the plugin pages answer from in place of the Nodes; nil
// otherwise. It is held in memory and nothing of it reaches a Node.
func (r *Runtime) pluginUsageSample() *plugins.UsageSample {
	ledger := r.localRuntime().App.DemoUsage()
	if ledger == nil {
		return nil
	}
	return &plugins.UsageSample{Report: ledger.Report, Inventory: ledger.Inventory}
}

// pluginUsesUsage reports whether a mount is a page that may read usage: a
// workspace or settings page, the scopes the usage calls accept, of a package
// that declares the usage action.
func pluginUsesUsage(mounted *pluginMountedView) bool {
	if kind := mounted.scope.Surface.Kind; kind != "workspace" && kind != "settings" {
		return false
	}
	return mounted.info.Extension != nil && slices.Contains(mounted.info.Extension.Actions, protocol.PluginActionReadUsage)
}

// demoChanged follows Demo mode going on, off or into another language. A
// page that reads usage is disposed and mounts again on the next frame that
// shows it, with the other ledger and in its initial state: a filter, range
// or page of records chosen over one data set means nothing in the other.
// Its host closes, which ends the calls that wait on it, and the new mount
// takes the bundle the old one read, so the Node is asked nothing. Every
// other mount stays and only has its host answer usage from the other
// ledger.
func (r *Runtime) demoChanged() {
	root := r.localRuntime()
	if root.pluginViews == nil {
		return
	}
	sample := root.pluginUsageSample()
	for key, mounted := range root.pluginViews.mounts {
		if pluginUsesUsage(mounted) {
			if mounted.bundle != nil {
				if root.pluginViews.remounts == nil {
					root.pluginViews.remounts = map[pluginMountKey]protocol.PluginDesktopBundle{}
				}
				root.pluginViews.remounts[key] = *mounted.bundle
			}
			root.dropPluginView(mounted, true)
			delete(root.pluginViews.mounts, key)
		} else if mounted.host != nil {
			mounted.host.SetUsageSample(sample)
		}
	}
	root.wake()
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
