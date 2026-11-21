package live

import (
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func overviewFixtureAsset(sequence uint64, path string) protocol.ConversationAssetsGroup {
	return protocol.ConversationAssetsGroup{Sequence: sequence, Worktree: protocol.WorktreeID{1}, Kind: protocol.ConversationAssetsKindArtifact,
		Items: []protocol.ConversationAssetsTarget{{Kind: "file", Data: protocol.ConversationAssetsTargetFilePayload{Path: path}}}}
}

func applyOverviewCallbacks(r *Runtime, c *ui.Context) {
	r.mu.Lock()
	queue := r.queue
	r.queue = nil
	r.mu.Unlock()
	for _, callback := range queue {
		callback(c)
	}
}

func TestOverviewIncrementalWindow(t *testing.T) {
	var read overviewRead
	groups := make([]protocol.ConversationAssetsGroup, overviewLimit)
	for i := range groups {
		groups[i] = overviewFixtureAsset(uint64(100-i), "existing.txt")
	}
	read.applyPage(protocol.ConversationAssetsPage{Groups: groups, Checkpoint: "cold"}, false)
	read.applyPage(protocol.ConversationAssetsPage{Delta: true, Checkpoint: "quiet"}, false)
	if len(read.groups) != overviewLimit || read.before != nil || read.checkpoint != "quiet" {
		t.Fatal("empty delta lost the retained window")
	}
	read.applyPage(protocol.ConversationAssetsPage{Groups: []protocol.ConversationAssetsGroup{overviewFixtureAsset(110, "new.txt")}, Delta: true, Checkpoint: "updated"}, false)
	if len(read.groups) != overviewLimit || read.groups[0].Sequence != 110 || read.groups[1].Sequence != 100 || read.before == nil || *read.before != read.groups[overviewLimit-1].Sequence {
		t.Fatal("delta did not preserve a bounded, resumable window")
	}
	read.applyPage(protocol.ConversationAssetsPage{Groups: []protocol.ConversationAssetsGroup{overviewFixtureAsset(55, "earlier.txt")}}, true)
	if len(read.groups) != overviewLimit+1 || read.before != nil {
		t.Fatal("older page did not extend the displayed window")
	}
	read.applyPage(protocol.ConversationAssetsPage{}, false)
	if len(read.groups) != 0 || read.checkpoint != "" || read.before != nil {
		t.Fatal("full fallback did not replace the retained window")
	}
}

func TestOverviewReadCheckpoint(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, _, id := cacheTestRuntime(t)
		var received []string
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "list_conversation_assets" {
				return link.Response{}, false
			}
			query := request.Command.Data.(protocol.CommandListConversationAssetsPayload).Query
			if query.Checkpoint == nil {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest}}, true
			}
			received = append(received, *query.Checkpoint)
			page := protocol.ConversationAssetsPage{Session: id, Revision: 1}
			switch *query.Checkpoint {
			case "":
				cursor := uint64(50)
				page.Checkpoint, page.NextBefore = "first", &cursor
			case "first":
				page.Groups, page.Checkpoint = []protocol.ConversationAssetsGroup{overviewFixtureAsset(1, "old.pdf")}, "window"
			case "window":
				page.Groups, page.Checkpoint, page.Delta = []protocol.ConversationAssetsGroup{overviewFixtureAsset(100, "new.pdf")}, "refreshed", true
			}
			return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: page}}, true
		}
		var read overviewRead
		for range 2 {
			page, err := r.readConversationAssets(t.Context(), id, nil, read.checkpoint)
			if err != nil {
				t.Fatal(err)
			}
			read.applyPage(page, false)
		}
		if len(received) != 3 || received[0] != "" || received[1] != "first" || received[2] != "window" || read.checkpoint != "refreshed" || len(read.groups) != 2 || read.groups[0].Sequence != 100 || read.groups[1].Sequence != 1 {
			t.Fatalf("checkpoint negotiation: tokens=%v window=%+v", received, read)
		}
	})
}

func TestOverviewSparseReadBudget(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, _, id := cacheTestRuntime(t)
		calls := 0
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "list_conversation_assets" {
				return link.Response{}, false
			}
			calls++
			before := uint64(100 - calls)
			return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: protocol.ConversationAssetsPage{Session: id, Revision: 1, NextBefore: &before}}}, true
		}
		page, err := r.readConversationAssets(t.Context(), id, nil, "")
		if err != nil || calls != overviewScanPages || len(page.Groups) != 0 || page.NextBefore == nil || *page.NextBefore != uint64(100-calls) {
			t.Fatalf("sparse read: calls=%d page=%+v error=%v", calls, page, err)
		}
	})
}

func TestSessionOverviewBackgroundRefresh(t *testing.T) {
	for _, populated := range []bool{false, true} {
		name := "empty inventory stays empty while refreshing"
		if populated {
			name = "existing outputs stay visible while refreshing"
		}
		t.Run(name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source, cv, id := cacheTestRuntime(t)
				state := cacheTestFrame(t, r, cv, id)
				read := &overviewRead{open: true, loaded: true, revision: state.Snapshot.Page.Revision, inventory: state.InventoryVersion,
					loadedRevision: state.Snapshot.Page.Revision, loadedInventory: state.InventoryVersion}
				if populated {
					read.groups = []protocol.ConversationAssetsGroup{overviewFixtureAsset(1, "existing.txt")}
				}
				r.overviews = map[protocol.SessionID]*overviewRead{id: read}
				started, proceed := make(chan struct{}), make(chan struct{})
				release := sync.OnceFunc(func() { close(proceed) })
				defer func() { release(); r.workers.Wait() }()
				source.handle = func(request protocol.Request) (link.Response, bool) {
					if request.Command.Kind != "list_conversation_assets" {
						return link.Response{}, false
					}
					close(started)
					<-proceed
					return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: protocol.ConversationAssetsPage{
						Session: id, Revision: read.revision, Groups: []protocol.ConversationAssetsGroup{overviewFixtureAsset(2, "generated.txt")},
					}}}, true
				}
				// A canonical publication adds an output without replacing history.
				state.Snapshot.Sequence++
				state.InventoryVersion++
				r.beginConversationFrame()
				r.conversationVersion++
				r.conversationCopies[id] = conversationCopy{state: state, ok: true, generation: r.conversationVersion}
				pending := r.sessionOverview(nil, cv)
				<-started
				if !read.loading || pending.Loading || pending.Failed || len(pending.Output) != len(read.groups) {
					t.Fatalf("background refresh replaced the displayed inventory: loading=%v, view=%+v", read.loading, pending)
				}
				if populated && pending.Output[0].Name != "existing.txt" {
					t.Fatal("background refresh discarded the previous output")
				}
				release()
				synctest.Wait()
				applyOverviewCallbacks(r, nil)
				updated := r.sessionOverview(nil, cv)
				if read.loading || updated.Loading || updated.Failed || !read.current() || len(updated.Output) != 1 || updated.Output[0].Name != "generated.txt" {
					t.Fatalf("completed refresh did not publish the new inventory: %+v", updated)
				}
			})
		})
	}
}

func TestSessionOverviewForegroundReads(t *testing.T) {
	for _, kind := range []string{"initial", "paging", "retry"} {
		t.Run(kind, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source, cv, id := cacheTestRuntime(t)
				state := cacheTestFrame(t, r, cv, id)
				read := &overviewRead{revision: state.Snapshot.Page.Revision, inventory: state.InventoryVersion}
				if kind != "initial" {
					read.loaded, read.loadedRevision, read.loadedInventory = true, read.revision, read.inventory
					read.groups = []protocol.ConversationAssetsGroup{overviewFixtureAsset(2, "existing.txt")}
				}
				if kind == "paging" {
					before := uint64(2)
					read.before = &before
					read.loadedInventory-- // an older page cannot certify that the head is current
				}
				read.failed = kind == "retry"
				r.overviews = map[protocol.SessionID]*overviewRead{id: read}
				started, proceed := make(chan struct{}), make(chan struct{})
				release := sync.OnceFunc(func() { close(proceed) })
				defer func() { release(); r.workers.Wait() }()
				source.handle = func(request protocol.Request) (link.Response, bool) {
					if request.Command.Kind != "list_conversation_assets" {
						return link.Response{}, false
					}
					close(started)
					<-proceed
					return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: protocol.ConversationAssetsPage{
						Session: id, Revision: read.revision, Groups: []protocol.ConversationAssetsGroup{overviewFixtureAsset(1, "loaded.txt")},
					}}}, true
				}
				view := r.sessionOverview(nil, cv)
				if kind == "retry" {
					state.Snapshot.Sequence++
					r.beginConversationFrame()
					r.conversationCopies[id] = conversationCopy{state: state, ok: true}
					view = r.sessionOverview(nil, cv)
					if !view.Failed {
						t.Fatal("streaming hid an inventory failure before retry")
					}
				}
				if kind == "initial" {
					view.Opened(true)
				} else {
					view.Load(kind == "paging")
				}
				<-started
				pending := r.sessionOverview(nil, cv)
				if !pending.Loading || pending.Failed || !read.loading {
					t.Fatalf("foreground read lost its loading indication: %+v", pending)
				}
				release()
				synctest.Wait()
				applyOverviewCallbacks(r, nil)
				updated := r.sessionOverview(nil, cv)
				if updated.Loading || updated.Failed || len(updated.Output) == 0 || updated.Output[len(updated.Output)-1].Name != "loaded.txt" {
					t.Fatalf("foreground read did not publish its result: %+v", updated)
				}
				if kind == "paging" && read.current() {
					t.Fatal("paging marked a stale inventory head as refreshed")
				}
			})
		})
	}
}

func TestSessionOverviewInvalidatedInventory(t *testing.T) {
	for _, test := range []struct {
		name       string
		paging     bool
		fault      protocol.ErrorCode
		keep       bool
		restart    bool
		retryFault protocol.ErrorCode
	}{
		{name: "missing checkpoint dependency", fault: protocol.ErrorCodeNotFound},
		{name: "permission revoked", fault: protocol.ErrorCodePermissionDenied},
		{name: "temporary transport failure", fault: protocol.ErrorCodeUnavailable, keep: true},
		{name: "rewound paging cursor", paging: true, fault: protocol.ErrorCodeWrongTarget, restart: true},
		{name: "paging conflict then missing head", paging: true, fault: protocol.ErrorCodeRevisionConflict, restart: true, retryFault: protocol.ErrorCodeNotFound},
		{name: "invalid cursor again during recovered pagination", paging: true, fault: protocol.ErrorCodeWrongTarget, restart: true, retryFault: protocol.ErrorCodeWrongTarget},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source, cv, id := cacheTestRuntime(t)
				state := cacheTestFrame(t, r, cv, id)
				revision, inventory := state.Snapshot.Page.Revision, state.InventoryVersion
				before := uint64(50)
				read := &overviewRead{open: true, loaded: true, revision: revision, loadedRevision: revision, inventory: inventory, loadedInventory: inventory,
					groups: []protocol.ConversationAssetsGroup{overviewFixtureAsset(60, "old.pdf")}, before: &before, checkpoint: "retained"}
				r.overviews = map[protocol.SessionID]*overviewRead{id: read}
				r.initAttachments()
				defer r.closeAttachments()
				resume := make(chan struct{})
				release := sync.OnceFunc(func() { close(resume) })
				defer release()
				var queries []protocol.ConversationAssetsQuery
				source.handle = func(request protocol.Request) (link.Response, bool) {
					if request.Command.Kind != "list_conversation_assets" {
						return link.Response{}, false
					}
					queries = append(queries, request.Command.Data.(protocol.CommandListConversationAssetsPayload).Query)
					fault := test.fault
					if len(queries) > 1 {
						<-resume
						fault = test.retryFault
					}
					if len(queries) == 2 && fault == protocol.ErrorCodeWrongTarget {
						cursor := uint64(20)
						return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: protocol.ConversationAssetsPage{Session: id, Revision: revision, NextBefore: &cursor}}}, true
					}
					if fault != "" {
						return link.Response{Fault: &protocol.Fault{Code: fault}}, true
					}
					return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: protocol.ConversationAssetsPage{
						Session: id, Revision: revision, Groups: []protocol.ConversationAssetsGroup{overviewFixtureAsset(10, "rewound.pdf")}, Checkpoint: "new",
					}}}, true
				}
				window := ui.NewTester(func(c *ui.Context) { applyOverviewCallbacks(r, c) }, 320, 200)
				view := r.sessionOverview(nil, cv)
				oldID := view.Output[0].ID
				view.Load(test.paging)
				r.workers.Wait()
				window.Frame()
				current := r.sessionOverview(nil, cv)
				if test.keep {
					if !current.Failed || !current.More || len(current.Output) != 1 || current.Output[0].ID != oldID || read.checkpoint != "retained" {
						t.Fatalf("transient error discarded the inventory: %+v", current)
					}
				} else {
					if len(current.Output) != 0 || current.More || read.loaded || read.checkpoint != "" || current.Loading != test.restart || current.Failed == test.restart {
						t.Fatalf("invalid inventory survived the failed read: %+v", current)
					}
					view.OpenAsset(oldID)
					if len(r.attachments.files) != 0 {
						t.Fatal("a queued click reopened an invalidated asset")
					}
				}
				release()
				r.workers.Wait()
				window.Frame()
				r.workers.Wait()
				final := r.sessionOverview(nil, cv)
				wantCalls := 1
				if test.restart {
					wantCalls = 2
					if test.retryFault == protocol.ErrorCodeWrongTarget {
						wantCalls++ // the recovered head's own continuation
					}
					if test.retryFault == "" {
						if !read.current() || len(final.Output) != 1 || final.Output[0].Name != "rewound.pdf" || read.checkpoint != "new" {
							t.Fatalf("fresh head did not replace the old page: %+v", final)
						}
					} else if !final.Failed || read.loading || len(final.Output) != 0 {
						t.Fatalf("failed restart retained assets or kept loading: %+v", final)
					}
				}
				if len(queries) != wantCalls || (queries[0].Before != nil) != test.paging || queries[0].Checkpoint == nil || *queries[0].Checkpoint != "retained" {
					t.Fatalf("unexpected asset reads: %+v", queries)
				}
				if test.restart && (queries[1].Before != nil || queries[1].Checkpoint == nil || *queries[1].Checkpoint != "") {
					t.Fatalf("restart reused the stale cursor or checkpoint: %+v", queries[1])
				}
			})
		})
	}
}

func TestSessionOverviewCoalescesStreamAndReleasesUnmountedRead(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, cv, id := cacheTestRuntime(t)
		state := cacheTestFrame(t, r, cv, id)
		revision := state.Snapshot.Page.Revision
		read := &overviewRead{open: true, loaded: true, revision: revision, inventory: state.InventoryVersion,
			loadedRevision: revision, loadedInventory: state.InventoryVersion}
		r.overviews = map[protocol.SessionID]*overviewRead{id: read}
		proceed := make(chan struct{})
		defer func() { close(proceed); r.workers.Wait() }()
		var calls atomic.Int32
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "list_conversation_assets" {
				return link.Response{}, false
			}
			calls.Add(1)
			<-proceed
			return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: protocol.ConversationAssetsPage{Session: id, Revision: revision}}}, true
		}
		advance := func(commit bool) {
			state.Snapshot.Sequence++
			if commit {
				state.InventoryVersion++
			}
			r.beginConversationFrame()
			r.conversationVersion++
			r.conversationCopies[id] = conversationCopy{state: state, ok: true, generation: r.conversationVersion}
			r.sessionOverview(nil, cv)
		}
		for range 200 {
			advance(false)
		}
		synctest.Wait()
		if calls.Load() != 0 || !read.current() {
			t.Fatal("draft deltas invalidated the loaded inventory")
		}
		advance(true)
		for range 20 {
			advance(false)
		}
		synctest.Wait()
		if calls.Load() != 1 {
			t.Fatalf("canonical commit started %d reads", calls.Load())
		}
		proceed <- struct{}{}
		synctest.Wait()
		applyOverviewCallbacks(r, nil)
		synctest.Wait()
		if calls.Load() != 1 || !read.current() {
			t.Fatal("deltas during an inventory read scheduled another scan")
		}
		for range 20 {
			advance(true)
		}
		synctest.Wait()
		if calls.Load() != 2 {
			t.Fatalf("stream burst caused %d concurrent inventory reads", calls.Load())
		}
		proceed <- struct{}{}
		synctest.Wait()
		applyOverviewCallbacks(r, nil)
		synctest.Wait()
		if calls.Load() != 3 {
			t.Fatalf("expected one follow-up, got %d total reads", calls.Load())
		}
		proceed <- struct{}{}
		synctest.Wait()
		applyOverviewCallbacks(r, nil)
		if !read.current() {
			t.Fatal("follow-up did not cover the latest stream head")
		}
		advance(true)
		synctest.Wait()
		r.beginConversationFrame()
		r.finishConversationFrame()
		proceed <- struct{}{}
		synctest.Wait()
		applyOverviewCallbacks(r, nil)
		if r.overviews[id] != nil || read.loading || read.current() {
			t.Fatal("unmounted inventory published a stale result")
		}
	})
}
