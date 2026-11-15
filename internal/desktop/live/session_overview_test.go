package live

import (
	"sync"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func overviewFixtureAsset(sequence uint64, path string) protocol.ConversationAssetsGroup {
	return protocol.ConversationAssetsGroup{Sequence: sequence, Worktree: protocol.WorktreeID{1}, Kind: protocol.ConversationAssetsKindArtifact,
		Items: []protocol.ConversationAssetsTarget{{Kind: "file", Data: protocol.ConversationAssetsTargetFilePayload{Path: path}}}}
}

func applyOverviewCallbacks(r *Runtime) {
	r.mu.Lock()
	queue := r.queue
	r.queue = nil
	r.mu.Unlock()
	for _, callback := range queue {
		callback(nil)
	}
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
				read := &overviewRead{open: true, loaded: true, revision: state.Snapshot.Page.Revision, head: state.Snapshot.Sequence,
					loadedRevision: state.Snapshot.Page.Revision, loadedHead: state.Snapshot.Sequence}
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
				// New text/history events advance the stream without replacing its revision.
				state.Snapshot.Sequence++
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
				applyOverviewCallbacks(r)
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
				read := &overviewRead{revision: state.Snapshot.Page.Revision, head: state.Snapshot.Sequence}
				if kind != "initial" {
					read.loaded, read.loadedRevision, read.loadedHead = true, read.revision, read.head
					read.groups = []protocol.ConversationAssetsGroup{overviewFixtureAsset(2, "existing.txt")}
				}
				if kind == "paging" {
					before := uint64(2)
					read.before = &before
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
				applyOverviewCallbacks(r)
				updated := r.sessionOverview(nil, cv)
				if updated.Loading || updated.Failed || len(updated.Output) == 0 || updated.Output[len(updated.Output)-1].Name != "loaded.txt" {
					t.Fatalf("foreground read did not publish its result: %+v", updated)
				}
			})
		})
	}
}
