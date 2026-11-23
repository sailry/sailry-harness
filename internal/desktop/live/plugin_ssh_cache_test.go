package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type sshCacheLiveTestNode struct {
	snapshot protocol.Snapshot
	bundle   protocol.PluginDesktopBundle
	browses  atomic.Int32
	updates  chan protocol.Update
}

func (source *sshCacheLiveTestNode) Open(context.Context, protocol.NodeID, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture streams are unavailable")
}

func (source *sshCacheLiveTestNode) Dispatch(_ context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	ch := make(chan link.Response, 1)
	switch request.Command.Kind {
	case "read_plugin_view":
		ch <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: source.bundle}}
	case "browse_ssh_directory":
		count := source.browses.Add(1)
		ch <- link.Response{Output: protocol.Output{
			Kind: "ssh_outcome",
			Data: protocol.SshOutcome{
				Kind: "directory",
				Data: protocol.SshDirectory{
					Path:    "/root",
					Entries: []protocol.FileEntry{{Name: fmt.Sprintf("cached-%d.txt", count), Kind: protocol.EntryKindFile}},
				},
			},
		}}
	default:
		return link.Admission{}, fmt.Errorf("unexpected command %s", request.Command.Kind)
	}
	close(ch)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: ch}, nil
}

func (source *sshCacheLiveTestNode) Subscribe(_ context.Context, _ protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "node" {
		return nil, errors.New("fixture supports only Node observation")
	}
	initial := source.snapshot
	return &pluginViewTestSubscription{initial: &initial, closed: make(chan struct{}), updates: source.updates}, nil
}

func TestSshDirectoryCacheSurvivesPanelSwitchAndClearsOnClose(t *testing.T) {
	const entry = "dev.sailry.platform/desktop/main.js"
	profileID := protocol.NewSshID()
	info := protocol.DefaultPluginInfo()
	info.Summary.Name = "SSH Cache Fixture"
	info.Summary.Digest = strings.Repeat("d", 64)
	info.Summary.Revision = 1
	info.Summary.Enabled = true
	info.Summary.Compatibility.Kind = "compatible"
	ext := protocol.DefaultPluginExtension()
	ext.Scope = protocol.PluginScopeHost
	ext.Actions = []protocol.PluginAction{protocol.PluginActionReadSsh}
	desktop := protocol.DefaultPluginDesktopManifest()
	desktop.Entry = new(entry)
	desktop.Navigation = &protocol.PluginDesktopNavigation{Label: info.Summary.Name}
	desktop.NavigationOptions.Pinned = true
	ext.Desktop, info.Extension = &desktop, &ext

	node := protocol.NodeID{8}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Plugins = []protocol.PluginSummary{info.Summary}
	profileArg, _ := json.Marshal(profileID.String())
	source := &sshCacheLiveTestNode{
		snapshot: snapshot,
		updates:  make(chan protocol.Update, 2),
		bundle: protocol.PluginDesktopBundle{
			Package: packageReference(info.Summary),
			Entry:   entry,
			Files: map[string]string{
				entry: `
import {View, div} from "gpui-kit";
import {browseSshDirectory} from "sailry/connections";
export default class extends View {
  init(_props, cx) {
    this.label = "loading";
    cx.spawn(async cx => {
      const dir = await browseSshDirectory({profile: ` + string(profileArg) + `, expected_revision: 1, path: ".", after: null});
      this.label = dir.path + ":" + dir.entries[0].name;
      cx.notify();
    });
  }
  render() { return div().child(this.label); }
}`,
			},
		},
	}

	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(link.NewLocal(node, node, source))
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	root := &Runtime{
		ctx: ctx, cancel: cancel, controller: controller, packages: newPackageState(),
		busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
		App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}), Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
	}
	root.initPluginViews()
	t.Cleanup(func() {
		_ = root.Close()
		_ = controller.Close()
	})
	root.packages.infos[info.Summary.Name] = info
	root.App.Hooks.BeforeFrame = func(view *ui.Context) {
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(view)
		}
		root.pluginViewsFrame(view)
	}
	root.App.Hooks.AfterFrame = root.pluginViewsAfterFrame
	window := ui.NewTesterWith(root.App.View, 800, 500, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	window.SetLocale("en")
	window.Frame()

	waitForText := func(want string) {
		t.Helper()
		deadline := time.Now().Add(3 * time.Second)
		for time.Now().Before(deadline) {
			root.workers.Wait()
			window.Frame()
			if window.HasText(want) {
				return
			}
			time.Sleep(5 * time.Millisecond)
		}
		t.Fatalf("did not render %q; got %v", want, window.Texts())
	}

	// Open SSH page the first time -> 1 browse RPC.
	if err := window.Click(info.Summary.Name); err != nil {
		t.Fatal(err)
	}
	waitForText("/root:cached-1.txt")
	if got := source.browses.Load(); got != 1 {
		t.Fatalf("initial browse count = %d, want 1", got)
	}
	if root.pluginViews.sshDirectories.Len() == 0 {
		t.Fatal("expected SSH directory cache to be populated")
	}

	// Switch away from the SSH page -> mount is disposed, cache is retained.
	if !root.App.ClosePage() {
		t.Fatal("failed to close SSH plugin page")
	}
	window.Frame()
	if len(root.pluginViews.mounts) != 0 {
		t.Fatalf("closed page kept %d mounts", len(root.pluginViews.mounts))
	}
	if root.pluginViews.sshDirectories.Len() == 0 {
		t.Fatal("closing navigation page prematurely cleared SSH directory cache")
	}

	// Switch back to SSH page -> remounts and renders immediately from cache with 0 new RPCs.
	if err := window.Click(info.Summary.Name); err != nil {
		t.Fatal(err)
	}
	waitForText("/root:cached-1.txt")
	if got := source.browses.Load(); got != 1 {
		t.Fatalf("returning to SSH page triggered %d browse RPCs, want 1", got)
	}

	// Closing the runtime releases the owned SSH directory cache.
	if err := root.Close(); err != nil {
		t.Fatal(err)
	}
	if got := root.pluginViews.sshDirectories.Len(); got != 0 {
		t.Fatalf("Close left %d cached SSH directory entries", got)
	}
}
