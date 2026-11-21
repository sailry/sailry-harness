package client

import (
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func conversationObserverFixture(t testing.TB, snapshot protocol.ConversationSnapshot) (*conversationObserver, *conversationWatch) {
	t.Helper()
	observer := newObserver(t.Context())
	t.Cleanup(func() { _ = observer.Close() })
	session := snapshot.Page.Session
	watch := &conversationWatch{session: session, ctx: observer.ctx, projection: NewConversationProjection(snapshot.Node, session, 1)}
	client := &Client{conversations: map[protocol.SessionID]*conversationWatch{session: watch}}
	owner := &conversationObserver{
		client: client, observer: observer, session: session, referenceRows: true,
		reads: map[string]*detailRead{}, failures: map[string]*protocol.Fault{}, sent: map[string]bool{},
	}
	observer.read, observer.control = owner.view, owner.control
	applyObserverUpdate(t, owner, watch, protocol.Update{Kind: "conversation_snapshot", Data: snapshot})
	return owner, watch
}

func applyObserverUpdate(t testing.TB, owner *conversationObserver, watch *conversationWatch, update protocol.Update) {
	t.Helper()
	if got, err := owner.client.applyConversationUpdate(watch, watch.projection.generation, update); got != Applied || err != nil {
		t.Fatalf("observer update %s = %v, %v", update.Kind, got, err)
	}
}

func readObserverView(t *testing.T, owner *conversationObserver) map[string]any {
	t.Helper()
	value, err := owner.view()
	if err != nil {
		t.Fatal(err)
	}
	return value.(map[string]any)
}

func TestConversationObserverReusesDetachedViewAcrossReadsAndControl(t *testing.T) {
	snapshot := conversationFixture()
	turn := snapshot.Page.Runs[0].Turn
	snapshot.Page.Entries = append(snapshot.Page.Entries, protocol.ConversationEntry{
		ID: "result", Turn: turn, Sequence: 2, Author: "assistant",
		Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{
			Name: "example", Summary: &protocol.ToolDetailSummary{Revision: "revision", Published: true, HasDetails: true},
		}}},
	})
	owner, watch := conversationObserverFixture(t, snapshot)
	ref := *ToolCalls(snapshot.Page)[0].Reference
	key := detailKey(ref)
	detail := protocol.DefaultToolDetail()
	detail.Result = new(json.RawMessage(`"result detail"`))
	owner.client.conversationDetails = map[string]conversationDetail{key: {reference: ref, value: detail}}
	owner.failures[key] = observerFault(fault(protocol.ErrorCodeUnavailable, "retry the detail"))
	track := func() {
		t.Helper()
		selection := json.RawMessage(`{"items":[{"entry":"result","index":0}],"retry":[{"entry":"result","index":0}]}`)
		if err := owner.observer.Control(t.Context(), "track_tools", selection); err != nil {
			t.Fatal(err)
		}
	}
	track()
	first := owner.previous
	if first == nil || len(first.derived.calls) != 1 || owner.failures[key] != nil {
		t.Fatal("tool selection did not prepare a detached view and clear the retried fault")
	}
	transcript := first.derived
	initial := readObserverView(t, owner)
	initialCall := initial["calls"].([]map[string]any)[0]
	if initial["snapshot"] != &first.Snapshot || owner.previous.derived != transcript || initialCall["detail"] == nil {
		t.Fatal("read did not reuse the control's snapshot and deliver the ready detail")
	}
	canonical := &watch.projection.snapshot.Page.Entries[1].Parts[0]
	if &first.Snapshot.Page.Entries[1].Parts[0] == canonical || transcript.calls[0].Summary == canonical.Data.(protocol.ConversationPartToolResultPayload).Summary {
		t.Fatal("observer cached a mutable canonical tool entry")
	}

	applyObserverUpdate(t, owner, watch, conversationFrame(*watch.projection.snapshot, "statistics", protocol.ConversationStatistics{Turns: 2}))
	track()
	controlled := owner.previous
	updated := readObserverView(t, owner)
	stats := updated["snapshot"].(*protocol.ConversationSnapshot)
	if stats != &controlled.Snapshot || stats.Sequence != first.Snapshot.Sequence+1 || stats.Statistics == nil || stats.Statistics.Turns != 2 || first.Snapshot.Statistics != nil {
		t.Fatal("statistics left stale metadata or changed an earlier packet")
	}
	if &stats.Page.Entries[0] != &first.Snapshot.Page.Entries[0] || &stats.Page.Runs[0] != &first.Snapshot.Page.Runs[0] ||
		owner.previous.derived != transcript || &updated["timeline"].([]protocol.TurnID)[0] != &initial["timeline"].([]protocol.TurnID)[0] {
		t.Fatal("statistics copied or recomputed the detached transcript")
	}
	if call := updated["calls"].([]map[string]any)[0]; call["detail_state"].(map[string]any)["kind"] != "ready" {
		t.Fatal("cached tool detail stopped being ready")
	} else if _, present := call["detail"]; present || initialCall["detail"] == nil {
		t.Fatal("the ready detail was resent or the first response changed")
	}

	watch.state.Connected = false
	disconnected := readObserverView(t, owner)
	if disconnected["connected"] != false || disconnected["snapshot"] != stats || owner.previous.derived != transcript {
		t.Fatal("a connection-only update did not reuse the detached snapshot and transcript")
	}
	mutable, _ := owner.client.Conversation(owner.session)
	mutable.Snapshot.Page.Entries[1].Parts[0].Data.(protocol.ConversationPartToolResultPayload).Summary.Revision = "changed"
	if transcript.calls[0].Summary.Revision != "revision" {
		t.Fatal("a mutable conversation read changed the observer's cached transcript")
	}

	applyObserverUpdate(t, owner, watch, conversationFrame(*watch.projection.snapshot, "statistics_failed", protocol.Fault{Code: protocol.ErrorCodeUnavailable}))
	failed := readObserverView(t, owner)
	if failed["statistics_error"].(*protocol.Fault).Code != protocol.ErrorCodeUnavailable || failed["snapshot"].(*protocol.ConversationSnapshot).Statistics != nil || owner.previous.derived != transcript {
		t.Fatal("statistics failure did not clear the totals independently of the transcript")
	}

	applyObserverUpdate(t, owner, watch, conversationFrame(*watch.projection.snapshot, "delta", protocol.ConversationDraft{
		ID: "draft", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Streaming"}},
	}))
	streaming := readObserverView(t, owner)["snapshot"].(*protocol.ConversationSnapshot)
	if len(streaming.Drafts) != 1 || len(stats.Drafts) != 0 || owner.previous.derived == transcript || &streaming.Page.Entries[0] != &stats.Page.Entries[0] {
		t.Fatal("a live draft reused obsolete rows or copied already detached entries")
	}
}

func TestConversationObserverArtifactLifetimeAndReset(t *testing.T) {
	snapshot := conversationFixture()
	turn := snapshot.Page.Runs[0].Turn
	owner, watch := conversationObserverFixture(t, snapshot)
	initial := readObserverView(t, owner)
	apply := func(kind string, data any) {
		t.Helper()
		applyObserverUpdate(t, owner, watch, conversationFrame(*watch.projection.snapshot, kind, data))
	}
	apply("entry", protocol.ConversationEntry{ID: "answer", Turn: turn, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Answer"}}})
	artifact := protocol.DefaultArtifactSummary()
	artifact.ID, artifact.Session, artifact.Turn, artifact.Revision = protocol.ArtifactID{1}, snapshot.Page.Session, turn, 1
	apply("artifact", artifact)
	published := readObserverView(t, owner)
	artifact.Revision++
	apply("artifact", artifact)
	revised := readObserverView(t, owner)
	if initial["live_start"] != nil || initial["artifact_cursor"] != uint64(1) || published["live_start"] != uint64(2) || published["artifact_cursor"] != uint64(2) || revised["artifact_cursor"] != uint64(3) {
		t.Fatal("the packet cursors did not follow the captured snapshots")
	}
	if published["snapshot"].(*protocol.ConversationSnapshot).Artifacts.Items[0].Revision != 1 || revised["snapshot"].(*protocol.ConversationSnapshot).Artifacts.Items[0].Revision != 2 {
		t.Fatal("an artifact frame changed an earlier packet or reused stale metadata")
	}

	if err := watch.projection.Reconnect(2); err != nil {
		t.Fatal(err)
	}
	if recovering := readObserverView(t, owner); recovering["generation"] != uint64(2) || recovering["snapshot"] != revised["snapshot"] {
		t.Fatal("reconnect did not retain the authorized page pending its replacement")
	}
	snapshot.Page.Entries[0].Parts[0].Data = "Replacement"
	applyObserverUpdate(t, owner, watch, protocol.Update{Kind: "conversation_snapshot", Data: snapshot})
	owner.changed()
	if owner.previous != nil {
		t.Fatal("a replacement epoch retained an idle observer's old cache")
	}
	reset := readObserverView(t, owner)
	fresh := reset["snapshot"].(*protocol.ConversationSnapshot)
	if reset["generation"] != uint64(2) || reset["artifact_cursor"] != uint64(4) || reset["live_start"] != nil || fresh.Sequence != snapshot.Sequence || len(fresh.Artifacts.Items) != 0 || fresh.Page.Entries[0].Parts[0].Data != "Replacement" {
		t.Fatal("a replacement snapshot retained data or metadata from the old epoch")
	}
	if segments := reset["segments"].(map[string]any)[turn.String()].([]transcriptSegment); segments[0].Inputs[0].Message.Text != "Replacement" {
		t.Fatal("a replacement snapshot reused old transcript rows")
	}

	watch.discardHistory()
	watch.state.Err = fault(protocol.ErrorCodePermissionDenied, "conversation access was revoked")
	owner.changed()
	if owner.previous != nil {
		t.Fatal("revocation retained an idle observer's view")
	}
	packet, err := owner.observer.Next(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(packet, &fields); err != nil {
		t.Fatal(err)
	}
	if string(fields["snapshot"]) != "null" || string(fields["calls"]) != "[]" || string(fields["segments"]) != "{}" {
		t.Fatal("loss of access did not clear the detached observer view")
	}
}

func BenchmarkConversationObserverStatistics(b *testing.B) {
	snapshot := conversationFixture()
	for i := 2; i <= 128; i++ {
		turn := protocol.TurnID{byte(i)}
		snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{
			Session: snapshot.Page.Session, Turn: turn, Sequence: uint64(i), Revision: 1, Kind: "task", Status: "completed",
		})
		snapshot.Page.Entries = append(snapshot.Page.Entries, protocol.ConversationEntry{
			ID: fmt.Sprintf("answer/%d", i), Turn: turn, Sequence: uint64(i), Author: "assistant",
			Parts: []protocol.ConversationPart{{Kind: "text", Data: "A synthetic answer from a completed turn"}},
		})
	}
	owner, watch := conversationObserverFixture(b, snapshot)
	if _, err := owner.view(); err != nil {
		b.Fatal(err)
	}
	stats := protocol.DefaultConversationStatistics()
	b.ReportAllocs()
	for b.Loop() {
		applyObserverUpdate(b, owner, watch, conversationFrame(*watch.projection.snapshot, "statistics", stats))
		if _, err := owner.view(); err != nil {
			b.Fatal(err)
		}
	}
}
