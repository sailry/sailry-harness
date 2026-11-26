package live

import (
	"slices"
	"sync"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	view "github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func newCapabilityTestNode(snap protocol.Snapshot, respond func(protocol.Request) link.Response) *terminalTabsTestNode {
	if respond == nil {
		respond = func(protocol.Request) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
		}
	}
	return &terminalTabsTestNode{
		pluginViewTestNode: pluginViewTestNode{snapshot: snap, updates: make(chan protocol.Update, 4)},
		respond:            respond,
	}
}

func TestHostCapabilitiesPropagationAndEditorGating(t *testing.T) {
	cloneLabel := i18n.In("en", "project_clone")
	repoLabel := i18n.In("en", "project_repository")
	hostFieldLabel := i18n.In("en", "project_host")

	t.Run("harness host with git hides clone", func(t *testing.T) {
		nodeID := protocol.NodeID{0x11}
		snap := protocol.DefaultSnapshot()
		snap.Node = nodeID
		snap.ProjectSources = []string{"local"}
		snap.HostTools = []protocol.HostTool{{Name: "git", Available: true}}

		source := newCapabilityTestNode(snap, nil)
		r, services, _ := newWindowServicesFixture(t, source, nil)
		tester := ui.NewTester(r.App.View, 1024, 720)
		tester.SetLocale("en")
		tester.Frame()

		nodes := services.Nodes()
		if len(nodes) != 1 || nodes[0].CanClone || !slices.Equal(nodes[0].ProjectSources, []string{"local"}) || len(nodes[0].Tools) != 1 || !nodes[0].Tools[0].Available {
			t.Fatalf("Nodes() = %+v", nodes)
		}
		if len(r.App.Data.Hosts) != 1 || r.App.Data.Hosts[0].CanClone || !slices.Equal(r.App.Data.Hosts[0].ProjectSources, []string{"local"}) {
			t.Fatalf("App.Data.Hosts = %+v", r.App.Data.Hosts)
		}

		r.App.Sidebar.OpenEditor(nil)
		tester.Frame()
		if tester.HasText(cloneLabel) {
			t.Fatalf("Harness Add Project unexpectedly showed %q: %q", cloneLabel, tester.Texts())
		}
	})

	t.Run("code host with git shows clone and code host without git hides clone", func(t *testing.T) {
		for _, tc := range []struct {
			name      string
			gitAvail  bool
			wantClone bool
		}{
			{"git available", true, true},
			{"git missing", false, false},
		} {
			t.Run(tc.name, func(t *testing.T) {
				nodeID := protocol.NodeID{0x22}
				snap := protocol.DefaultSnapshot()
				snap.Node = nodeID
				snap.ProjectSources = []string{"local", "clone"}
				snap.HostTools = []protocol.HostTool{{Name: "git", Available: tc.gitAvail}}

				source := newCapabilityTestNode(snap, nil)
				r, services, _ := newWindowServicesFixture(t, source, nil)
				tester := ui.NewTester(r.App.View, 1024, 720)
				tester.SetLocale("en")
				tester.Frame()

				nodes := services.Nodes()
				if len(nodes) != 1 || nodes[0].CanClone != tc.wantClone || !slices.Equal(nodes[0].ProjectSources, []string{"local", "clone"}) {
					t.Fatalf("Nodes() = %+v, want CanClone=%v", nodes, tc.wantClone)
				}
				if len(r.App.Data.Hosts) != 1 || r.App.Data.Hosts[0].CanClone != tc.wantClone {
					t.Fatalf("App.Data.Hosts = %+v, want CanClone=%v", r.App.Data.Hosts, tc.wantClone)
				}

				r.App.Sidebar.OpenEditor(nil)
				tester.Frame()
				if got := tester.HasText(cloneLabel); got != tc.wantClone {
					t.Fatalf("Add Project HasText(%q) = %v, want %v; texts=%q", cloneLabel, got, tc.wantClone, tester.Texts())
				}
			})
		}
	})

	t.Run("switching host resets clone to local and hides switcher", func(t *testing.T) {
		localID := protocol.NodeID{0x31}
		remoteID := protocol.NodeID{0x32}

		localSnap := protocol.DefaultSnapshot()
		localSnap.Node = localID
		localSnap.ProjectSources = []string{"local"}
		localSnap.HostTools = []protocol.HostTool{{Name: "git", Available: true}}

		remoteSnap := protocol.DefaultSnapshot()
		remoteSnap.Node = remoteID
		remoteSnap.ProjectSources = []string{"local", "clone"}
		remoteSnap.HostTools = []protocol.HostTool{{Name: "git", Available: true}}

		localSource := newCapabilityTestNode(localSnap, nil)
		remoteSource := newCapabilityTestNode(remoteSnap, nil)

		remoteClient := client.New(link.NewLocal(remoteID, remoteID, remoteSource))
		t.Cleanup(func() { _ = remoteClient.Close() })
		if err := remoteClient.Start(t.Context()); err != nil {
			t.Fatal(err)
		}

		r, services, _ := newWindowServicesFixture(t, localSource, nil)
		r.bindings = map[string]*Runtime{remoteID.String(): r.newBinding(remoteClient)}
		r.resourceCatalog = &protocol.ResourceCatalog{
			Entry: localID,
			Sources: []protocol.ResourceSource{{
				Node: remoteID,
				Name: "Code Remote",
			}},
		}
		tester := ui.NewTester(r.App.View, 1024, 720)
		tester.SetLocale("en")
		tester.Frame()

		nodes := services.Nodes()
		if len(nodes) != 2 || nodes[0].CanClone || !nodes[1].CanClone {
			t.Fatalf("Nodes() = %+v, want local CanClone=false and remote CanClone=true", nodes)
		}

		// OpenCloneEditor picks the clone-capable remote host.
		r.App.Sidebar.OpenCloneEditor()
		tester.Frame()
		if draft := r.App.Sidebar.Draft(); draft.Clone != 1 || !tester.HasText(cloneLabel) || !tester.HasText(repoLabel) {
			t.Fatalf("OpenCloneEditor did not start on clone-capable remote: draft=%+v texts=%q", draft, tester.Texts())
		}

		// Switch Host in Add Project to Local (Harness) -> resets to local and hides switcher.
		trigger, ok := tester.AccessByRole(ui.RoleMenuButton, hostFieldLabel)
		if !ok {
			t.Fatalf("no host trigger %q: %+v", hostFieldLabel, tester.AccessAll())
		}
		tester.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
		tester.Frame()
		localLabel := i18n.In("en", "local_host_name")
		all := tester.AccessAll()
		clicked := false
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Name == localLabel && (all[i].Role == ui.RoleMenuItem || all[i].Role == ui.RoleMenuItemRadio || all[i].Role == ui.RoleMenuItemCheckBox) {
				tester.ClickAt(all[i].Bounds.X+all[i].Bounds.W/2, all[i].Bounds.Y+all[i].Bounds.H/2)
				clicked = true
				break
			}
		}
		if !clicked {
			t.Fatalf("no host menu item %q: %+v", localLabel, all)
		}
		tester.Frame()
		tester.Frame()

		if draft := r.App.Sidebar.Draft(); draft.Clone != 0 || draft.Host != localLabel || tester.HasText(cloneLabel) || tester.HasText(repoLabel) {
			t.Fatalf("after switching to Harness local host: draft=%+v texts=%q", draft, tester.Texts())
		}
	})

	t.Run("inspect_host tools refresh and snapshot capability change notify OnChange", func(t *testing.T) {
		nodeID := protocol.NodeID{0x44}
		snap := protocol.DefaultSnapshot()
		snap.Node = nodeID
		snap.ProjectSources = []string{"local", "clone"}
		snap.HostTools = []protocol.HostTool{{Name: "git", Available: false}}

		gitPath := "/usr/bin/git"
		gitVersion := "git version 2.47.0"
		var inspectMu sync.Mutex
		inspectReady := false

		source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snap, updates: make(chan protocol.Update, 4)}}
		source.respond = func(req protocol.Request) link.Response {
			if req.Command.Kind == "inspect_host" {
				inspectMu.Lock()
				ready := inspectReady
				inspectMu.Unlock()
				info := protocol.DefaultHostInfo()
				info.Node = nodeID
				if ready {
					info.Tools = []protocol.HostTool{{Name: "git", Available: true, Path: &gitPath, Version: &gitVersion}}
				} else {
					info.Tools = []protocol.HostTool{{Name: "git", Available: false}}
				}
				return link.Response{Output: protocol.Output{Kind: "host_info", Data: info}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
		}

		r, services, _ := newWindowServicesFixture(t, source, nil)
		tester := ui.NewTester(r.App.View, 1024, 720)
		tester.SetLocale("en")
		tester.Frame()
		r.workers.Wait()
		tester.Frame()

		if nodes := services.Nodes(); len(nodes) != 1 || nodes[0].CanClone {
			t.Fatalf("initial Nodes() = %+v, want CanClone=false", nodes)
		}

		changeCh := make(chan struct{}, 8)
		unsub := services.OnChange(func() {
			select {
			case changeCh <- struct{}{}:
			default:
			}
		})
		defer unsub()

		// Simulate inspect_host discovering git while Snapshot still has the initial HostTools.
		inspectMu.Lock()
		inspectReady = true
		inspectMu.Unlock()
		state := r.hosts[nodeID.String()]
		if state == nil {
			t.Fatal("missing local liveHost state")
		}
		state.infoNext = tester.Now().Add(-1)
		r.readHostInfo(state)
		r.workers.Wait()
		tester.Frame()

		awaitWindowValue(t, changeCh)

		var got view.NodeInfo
		if nodes := services.Nodes(); len(nodes) != 1 {
			t.Fatalf("Nodes() after inspect_host = %+v", nodes)
		} else {
			got = nodes[0]
		}
		if !got.CanClone || len(got.Tools) != 1 || !got.Tools[0].Available || got.Tools[0].Path == nil || *got.Tools[0].Path != gitPath {
			t.Fatalf("Nodes()[0] after inspect_host = %+v", got)
		}
		if len(r.App.Data.Hosts) != 1 || !r.App.Data.Hosts[0].CanClone {
			t.Fatalf("App.Data.Hosts after inspect_host = %+v", r.App.Data.Hosts)
		}

		// Subsequent Snapshot update changing ProjectSources updates Nodes() and fires OnChange.
		updated := snap
		updated.Cursor++
		updated.ProjectSources = []string{"local"}
		updated.HostTools = []protocol.HostTool{{Name: "git", Available: true, Path: &gitPath, Version: &gitVersion}}
		source.updates <- protocol.Update{Kind: "snapshot", Data: updated}
		awaitWindowValue(t, changeCh)

		if nodes := services.Nodes(); len(nodes) != 1 || nodes[0].CanClone || !slices.Equal(nodes[0].ProjectSources, []string{"local"}) {
			t.Fatalf("Nodes() after snapshot update = %+v", nodes)
		}
		tester.Frame()
		if len(r.App.Data.Hosts) != 1 || r.App.Data.Hosts[0].CanClone || !slices.Equal(r.App.Data.Hosts[0].ProjectSources, []string{"local"}) {
			t.Fatalf("App.Data.Hosts after snapshot update = %+v", r.App.Data.Hosts)
		}
	})
}
