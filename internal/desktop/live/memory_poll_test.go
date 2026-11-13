package live

import (
	"context"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type memoryPollRead struct {
	ctx        context.Context
	request    protocol.Request
	completion chan link.Response
}

type memoryPollTransport struct {
	conversationTestTransport
	reads []*memoryPollRead
}

func (source *memoryPollTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "read_plugin_memory" {
		return source.conversationTestTransport.Dispatch(ctx, request)
	}
	read := &memoryPollRead{ctx: ctx, request: request, completion: make(chan link.Response, 1)}
	source.mu.Lock()
	source.reads = append(source.reads, read)
	source.mu.Unlock()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: read.completion}, nil
}

func (source *memoryPollTransport) captured() []*memoryPollRead {
	source.mu.Lock()
	defer source.mu.Unlock()
	return append([]*memoryPollRead(nil), source.reads...)
}

func memoryPollResponse(at time.Time, bytes uint64) link.Response {
	return link.Response{Output: protocol.Output{Kind: "plugin_memory", Data: protocol.PluginMemorySnapshot{
		CapturedAtMs: uint64(at.UnixMilli()),
		Samples: []protocol.PluginMemorySample{{
			Package: protocol.PluginReference{Name: "memory-fixture", Digest: strings.Repeat("a", 64)}, Kind: "tool",
			Bytes: &bytes, SampledAtMs: uint64(at.UnixMilli()), Instances: 1,
		}},
	}}}
}

func memoryPollFixture(t *testing.T, observe ...func(*ui.Context)) (*Runtime, *memoryPollTransport, *ui.Tester, func()) {
	t.Helper()
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Revision, info.Summary.Enabled = "memory-fixture", 1, true
	info.Summary.Digest = strings.Repeat("a", 64)
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	source := &memoryPollTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}}
	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(source)
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	r := &Runtime{ctx: ctx, cancel: cancel, controller: controller,
		App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	r.App.Page, r.App.Settings.Section = shell.SettingsPage, settings.Experimental
	// Exercise only the sampler and queued projections. A one-pixel frame
	// never starts the process-wide raster workers outside this test bubble.
	tester := ui.NewTesterWith(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		r.memoryMapFrame(c)
		for _, inspect := range observe {
			inspect(c)
		}
	}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	settle := func() {
		for range 3 {
			synctest.Wait()
			tester.Frame()
		}
		synctest.Wait()
	}
	t.Cleanup(func() {
		r.closePluginMemory()
		cancel()
		r.workers.Wait()
		_ = controller.Close()
	})
	return r, source, tester, settle
}

func assertMemoryPollValue(t *testing.T, r *Runtime, bytes uint64) {
	t.Helper()
	model := r.App.Settings.Model
	if len(model.PluginMemory) != 1 || len(model.PluginMemory[0].Items) != 1 || model.PluginMemory[0].Items[0].Value != float64(bytes) {
		t.Fatalf("plugin treemap does not project %d bytes: %+v", bytes, model.PluginMemory)
	}
	if len(model.PluginMemoryRows) != 1 || model.PluginMemoryRows[0].ID != "memory-fixture" || model.PluginMemoryRows[0].Total != host.Bytes(float64(bytes)) {
		t.Fatalf("plugin row does not project %d bytes: %+v", bytes, model.PluginMemoryRows)
	}
}

func TestPluginMemoryPolling(t *testing.T) {
	t.Run("five-second cadence and one in-flight read", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			r, source, window, settle := memoryPollFixture(t)
			settle()
			reads := source.captured()
			if len(reads) != 1 || reads[0].request.Command.Kind != "read_plugin_memory" || reads[0].request.Command.Durable() {
				t.Fatalf("initial diagnostic command = %+v", reads)
			}
			if _, ok := reads[0].request.Command.Data.(protocol.CommandReadPluginMemoryPayload); !ok {
				t.Fatalf("diagnostic command payload = %T", reads[0].request.Command.Data)
			}
			reads[0].completion <- memoryPollResponse(window.Now(), 1024)
			settle()
			assertMemoryPollValue(t, r, 1024)
			window.Advance(pluginMemoryInterval - time.Nanosecond)
			settle()
			if got := len(source.captured()); got != 1 {
				t.Fatalf("read repeated before five seconds: %d", got)
			}
			window.Advance(time.Nanosecond)
			settle()
			if got := len(source.captured()); got != 2 {
				t.Fatalf("read count at five seconds = %d, want 2", got)
			}
			window.Advance(3 * pluginMemoryInterval)
			settle()
			reads = source.captured()
			if len(reads) != 2 || r.pluginMemory == nil || !r.pluginMemory.pending {
				t.Fatalf("a pending read overlapped: reads %d, state %+v", len(reads), r.pluginMemory)
			}
			reads[1].completion <- memoryPollResponse(window.Now(), 2048)
			settle()
			assertMemoryPollValue(t, r, 2048)
			window.Advance(pluginMemoryInterval - time.Nanosecond)
			settle()
			if got := len(source.captured()); got != 2 {
				t.Fatalf("settling a held read caused an early refresh: %d", got)
			}
			window.Advance(time.Nanosecond)
			settle()
			if got := len(source.captured()); got != 3 {
				t.Fatalf("sampling did not resume at its next interval: %d", got)
			}
		})
	})
	t.Run("hidden page cancels and drops diagnostic demand", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			r, source, window, settle := memoryPollFixture(t)
			settle()
			source.captured()[0].completion <- memoryPollResponse(window.Now(), 1024)
			settle()
			assertMemoryPollValue(t, r, 1024)
			window.Advance(pluginMemoryInterval)
			settle()
			reads := source.captured()
			if len(reads) != 2 || !r.pluginMemory.pending {
				t.Fatalf("expected a held second request, got %d", len(reads))
			}
			r.App.Settings.Section = settings.General
			settle()
			if reads[1].ctx.Err() != context.Canceled || r.pluginMemory != nil || r.App.Settings.Model.Memory != nil || r.App.Settings.Model.PluginMemory != nil || r.App.Settings.Model.PluginMemoryRows != nil {
				t.Fatal("leaving Experimental retained its request or diagnostic values")
			}
			reads[1].completion <- memoryPollResponse(window.Now(), 99*1024)
			window.Advance(6 * pluginMemoryInterval)
			settle()
			if len(source.captured()) != 2 || r.pluginMemory != nil || r.App.Settings.Model.PluginMemoryRows != nil {
				t.Fatal("a hidden page read again or accepted a late result")
			}
			r.App.Settings.Section = settings.Experimental
			settle()
			reads = source.captured()
			if len(reads) != 3 || r.pluginMemory == nil || r.pluginMemory.backend != nil {
				t.Fatal("reopening reused the retired page's snapshot")
			}
			reads[2].completion <- memoryPollResponse(window.Now(), 2048)
			settle()
			assertMemoryPollValue(t, r, 2048)
		})
	})
	t.Run("queued completion cannot affect a new page lifetime", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			r, source, window, settle := memoryPollFixture(t)
			settle()
			old := r.pluginMemory
			source.captured()[0].completion <- memoryPollResponse(window.Now(), 99*1024)
			// The transport has completed, but its UI callback has not run.
			synctest.Wait()
			r.mu.Lock()
			queued := len(r.queue)
			r.mu.Unlock()
			if queued == 0 || old.backend != nil {
				t.Fatal("the fixture did not retain the completed read's UI callback")
			}
			// Close at the owner boundary before the queued callback, then
			// reopen at the same clock time with a new diagnostic state.
			r.App.Page = shell.ConversationPage
			r.closePluginMemory()
			r.App.Page = shell.SettingsPage
			settle()
			reads := source.captured()
			if len(reads) != 2 || r.pluginMemory == nil || r.pluginMemory == old || r.pluginMemory.backend != nil || old.backend != nil {
				t.Fatal("a queued result revived or mutated its retired diagnostic state")
			}
			reads[1].completion <- memoryPollResponse(window.Now(), 4096)
			settle()
			assertMemoryPollValue(t, r, 4096)
			if old.backend != nil {
				t.Fatal("the retired snapshot contaminated the current page")
			}
		})
	})
	for _, target := range []string{"page", "section"} {
		t.Run("queued failure after leaving "+target+" shows no toast", func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				toasted := false
				r, source, _, settle := memoryPollFixture(t, func(c *ui.Context) {
					// Inspect the actual toast queue without drawing its items or
					// starting raster workers in this synctest bubble.
					ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) { toasted = toasted || len(toasts) != 0 })
				})
				settle()
				old := r.pluginMemory
				source.captured()[0].completion <- link.Response{Fault: &protocol.Fault{
					Code: protocol.ErrorCodeInternal, Message: "Memory diagnostic failed",
				}}
				synctest.Wait()
				r.mu.Lock()
				queued := len(r.queue)
				r.mu.Unlock()
				if queued == 0 || old.error != "" {
					t.Fatal("the failed result did not wait for the UI frame")
				}
				if target == "page" {
					r.App.Page = shell.ConversationPage
				} else {
					r.App.Settings.Section = settings.General
				}
				// The queued callback runs before memoryMapFrame clears this
				// state, so its identity still matches when navigation changes.
				if r.pluginMemory != old {
					t.Fatal("navigation cleared the state before its queued callback")
				}
				settle()
				if toasted || old.error != "" || r.pluginMemory != nil || r.App.Settings.Model.PluginMemoryRows != nil || len(source.captured()) != 1 {
					t.Fatal("a queued failure escaped its page lifetime")
				}
			})
		})
	}
}

var _ link.Transport = (*memoryPollTransport)(nil)
