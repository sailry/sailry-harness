package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A completed update clears the card's Update offer without waiting for a
// projection input to change.
func TestPackageUpdateClearsOfferOnCompletion(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Revision, info.Summary.Version = "notes", "current", 7, new("1.0.0")
		info.Summary.Compatibility.Kind = "compatible"
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
		r, node := snapshotFixture(t, snapshot)
		r.packages.infos[info.Summary.Name] = info
		candidate := info
		candidate.Summary.Version = new("1.1.0")
		candidate.Origin = &protocol.PluginOrigin{Kind: "bundled"}
		r.packages.updates[info.Summary.Name] = protocol.PluginUpdatesReport{Current: info.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "package", Data: candidate}}
		r.projectPackages()
		if rows := r.App.Settings.Packages; len(rows) != 1 || !rows[0].UpdateAvailable {
			t.Fatalf("the update was not offered: %+v", rows)
		}
		node.respond = func(request protocol.Request) link.Response {
			return link.Response{Output: protocol.Output{Kind: "plugin", Data: info}}
		}
		finished := false
		r.updatePackage(info.Summary.Name, func() { finished = true })
		tt := pluginUpdateQueueTester(r)
		for range 2 {
			synctest.Wait()
			tt.Frame()
		}
		if len(node.sent("install_bundled_plugin")) != 1 || !finished {
			t.Fatalf("the update did not run: finished %v", finished)
		}
		if rows := r.App.Settings.Packages; len(rows) != 1 || rows[0].UpdateAvailable {
			t.Fatalf("the completed update is still offered: %+v", rows)
		}
	})
}
