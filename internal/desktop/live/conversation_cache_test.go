package live

import (
	"errors"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func cacheTestPage(session protocol.SessionID, number byte, text string) protocol.ConversationPage {
	page := protocol.DefaultConversationPage()
	page.Session, page.Revision = session, 7
	turn := protocol.TurnID{number}
	started, finished := int64(number)*1000, int64(number)*1000+500
	page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Sequence: uint64(number), Revision: 1, Kind: protocol.ConversationRunKindTask,
		Status: protocol.ConversationStatusCompleted, StartedMs: &started, FinishedMs: &finished}}
	page.Entries = []protocol.ConversationEntry{
		{ID: turn.String() + "/user", Turn: turn, Sequence: uint64(number)*2 - 1, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}}},
		{ID: turn.String() + "/reply", Turn: turn, Sequence: uint64(number) * 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Accepted reply"}}},
	}
	return page
}

func cacheTestRuntime(t *testing.T) (*Runtime, *liveWorkspaceBenchmarkTransport, *viewmodel.Conversation, protocol.SessionID) {
	t.Helper()
	id := protocol.SessionID{1}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	session := protocol.DefaultSession()
	session.ID, session.Revision = id, 1
	snapshot.Sessions = []protocol.Session{session}
	accepted := protocol.DefaultConversationSnapshot()
	accepted.Node, accepted.Sequence, accepted.Page = snapshot.Node, 10, cacheTestPage(id, 2, "Latest input")
	before := accepted.Page.Runs[0].Turn
	accepted.Page.NextBefore = &before
	source := &liveWorkspaceBenchmarkTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot},
		conversations: map[protocol.SessionID]protocol.ConversationSnapshot{id: accepted}}
	controller := client.New(source)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	cv := &viewmodel.Conversation{ID: id.String(), Node: snapshot.Node.String()}
	r := &Runtime{controller: controller, ctx: t.Context(), conversationEdits: map[string]conversationEdit{},
		composerOutgoing: map[string]*conversationOutgoing{}, App: shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})}
	release, err := controller.WatchConversation(t.Context(), id, func() { r.conversationChanged(id) })
	if err != nil {
		_ = controller.Close()
		t.Fatal(err)
	}
	t.Cleanup(func() { release(); _ = controller.Close() })
	synctest.Wait()
	return r, source, cv, id
}

func cacheTestFrame(t *testing.T, r *Runtime, cv *viewmodel.Conversation, id protocol.SessionID) client.ConversationState {
	t.Helper()
	r.beginConversationFrame()
	r.conversationSeen[id] = true
	state, ok := r.conversationState(id)
	if !ok || !state.Connected {
		t.Fatal("fixture conversation was not observed")
	}
	r.projectConversation(cv, id, state, time.Unix(20, 0), "en")
	r.finishConversationFrame()
	return state
}

func TestConversationFrameCachePagination(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, cv, id := cacheTestRuntime(t)
		first := cacheTestFrame(t, r, cv, id)
		retained := &cv.Turns[0]
		second := cacheTestFrame(t, r, cv, id)
		if &first.Snapshot.Page.Runs[0] != &second.Snapshot.Page.Runs[0] || retained != &cv.Turns[0] {
			t.Fatal("an unchanged frame recopied or reprojected completed history")
		}
		started, proceed := make(chan struct{}), make(chan struct{})
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "read_conversation" {
				return link.Response{}, false
			}
			close(started)
			<-proceed
			return link.Response{Output: protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{
				Sequence: first.Snapshot.Sequence, Page: cacheTestPage(id, 1, "Older input"),
			}}}, true
		}
		loaded := make(chan error, 1)
		go func() { loaded <- r.controller.LoadOlderConversation(t.Context(), id) }()
		<-started
		loading := cacheTestFrame(t, r, cv, id)
		if !loading.LoadingOlder || !cv.Session.LoadingOlder {
			t.Fatal("history loading did not invalidate the presentation")
		}
		close(proceed)
		if err := <-loaded; err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		after := cacheTestFrame(t, r, cv, id)
		if after.Snapshot.Sequence != first.Snapshot.Sequence || after.Snapshot.Page.Revision != first.Snapshot.Page.Revision ||
			len(cv.Turns) != 2 || cv.Turns[0].User != "Older input" || cv.Session.LoadingOlder {
			t.Fatalf("same-revision pagination stayed stale: %#v", cv)
		}
		r.beginConversationFrame()
		r.finishConversationFrame()
		if len(r.conversationCache) != 0 || len(r.conversationPresentations) != 0 || r.conversationCopies != nil {
			t.Fatal("leaving the conversation retained its frame caches")
		}
	})
}

func TestConversationFrameCacheKeepsUnchangedSibling(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, first, firstID := cacheTestRuntime(t)
		secondID := protocol.SessionID{2}
		second := &viewmodel.Conversation{ID: secondID.String(), Node: first.Node}
		observed := protocol.DefaultConversationSnapshot()
		observed.Node, observed.Sequence, observed.Page = source.snapshot.Node, 10, cacheTestPage(secondID, 3, "Second conversation")
		source.conversations[secondID] = observed
		release, err := r.controller.WatchConversation(t.Context(), secondID, func() { r.conversationChanged(secondID) })
		if err != nil {
			t.Fatal(err)
		}
		defer release()
		synctest.Wait()

		frame := func() (client.ConversationState, client.ConversationState) {
			r.beginConversationFrame()
			defer r.finishConversationFrame()
			r.conversationSeen[firstID], r.conversationSeen[secondID] = true, true
			one, firstOK := r.conversationState(firstID)
			two, secondOK := r.conversationState(secondID)
			if !firstOK || !secondOK || !one.Connected || !two.Connected || len(one.Snapshot.Page.Runs) == 0 || len(two.Snapshot.Page.Runs) == 0 {
				t.Fatal("both loaded conversations should be observed")
			}
			r.projectConversation(first, firstID, one, time.Unix(20, 0), "en")
			r.projectConversation(second, secondID, two, time.Unix(20, 0), "en")
			return one, two
		}
		oldFirst, oldSecond := frame()
		firstTurn := &first.Turns[0]
		observed.Sequence++
		observed.Page = cacheTestPage(secondID, 4, "Updated second conversation")
		observed.Page.Revision++
		source.conversations[secondID] = observed
		if err := r.controller.RefreshConversation(t.Context(), secondID); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()

		newFirst, newSecond := frame()
		if &oldFirst.Snapshot.Page.Runs[0] != &newFirst.Snapshot.Page.Runs[0] || firstTurn != &first.Turns[0] {
			t.Fatal("a sibling update recopied or reprojected unchanged history")
		}
		if &oldSecond.Snapshot.Page.Runs[0] == &newSecond.Snapshot.Page.Runs[0] || second.Turns[0].User != "Updated second conversation" {
			t.Fatal("the updated conversation retained stale history")
		}
	})
}

func TestConversationPresentationTransientState(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, cv, id := cacheTestRuntime(t)
		state := cacheTestFrame(t, r, cv, id)
		r.beginConversationFrame()
		apply := func(changed client.ConversationState, language string) {
			r.conversationVersion++
			r.conversationCopies[id] = conversationCopy{state: changed, ok: true, generation: r.conversationVersion}
			r.projectConversation(cv, id, changed, time.Unix(20, 0), language)
		}
		disconnected := state
		disconnected.Connected = false
		disconnected.Err = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "offline"}
		apply(disconnected, "en")
		if cv.Session.Connection != conversation.Reconnecting {
			t.Fatal("an observation outage retained the connected presentation")
		}
		loading := state
		loading.Loading, loading.LoadingOlder, loading.OlderErr = true, true, errors.New("older page failed")
		apply(loading, "en")
		if !cv.Session.Loading || !cv.Session.LoadingOlder || !cv.Session.OlderFailed {
			t.Fatal("loading and pagination failures did not refresh")
		}
		apply(state, "en")
		cv.Session.Connection = conversation.Offline
		r.projectConversation(cv, id, state, time.Unix(21, 0), "en")
		if cv.Session.Connection != conversation.Connected || cv.Session.Loading || cv.Session.OlderFailed {
			t.Fatal("a recovered Node retained transient diagnostics")
		}
		call := "localized"
		localized := state
		localized.Snapshot.Page.Entries = append([]protocol.ConversationEntry{}, state.Snapshot.Page.Entries...)
		localized.Snapshot.Page.Entries[1].Parts = []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{
			ID: &call, Name: "read_file", Summary: &protocol.ToolDetailSummary{Resolved: &protocol.ToolDetailResolved{Label: &protocol.ToolResultLocalText{
				Label: "Read file", Locales: map[string]string{"zh-CN": "Read file in Chinese"},
			}}},
		}}}
		apply(localized, "en")
		if cv.Turns[0].Reply.Blocks[0].Calls[0].Label != "Read file" {
			t.Fatal("the fixture has no localized tool label")
		}
		r.projectConversation(cv, id, localized, time.Unix(21, 0), "zh-CN")
		if cv.Turns[0].Reply.Blocks[0].Calls[0].Label != "Read file in Chinese" {
			t.Fatal("switching locale reused the previous tool projection")
		}
		active := state
		active.Snapshot.Page.Runs = append([]protocol.ConversationRun{}, state.Snapshot.Page.Runs...)
		active.Snapshot.Page.Runs[0].Status, active.Snapshot.Page.Runs[0].FinishedMs = protocol.ConversationStatusRunning, nil
		active.Snapshot.Drafts = []protocol.ConversationDraft{{ID: "draft", Turn: active.Snapshot.Page.Runs[0].Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Streaming reply"}}}}
		apply(active, "en")
		elapsed := cv.Turns[0].Reply.Elapsed
		r.projectConversation(cv, id, active, time.Unix(22, 0), "en")
		if cv.Turns[0].Reply.Elapsed != elapsed+2*time.Second || cv.Turns[0].Reply.Status != conversation.Running {
			t.Fatal("an unchanged live source stopped advancing its clock")
		}
	})
}

func TestConversationPresentationEditAndOutgoing(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, cv, id := cacheTestRuntime(t)
		state := cacheTestFrame(t, r, cv, id)
		r.beginConversationFrame()
		turn := state.Snapshot.Page.Runs[0].Turn
		r.conversationEdits[cv.ID] = conversationEdit{Turn: turn, Input: protocol.ConversationInput{Text: "Edited draft"}}
		comp := composer.New("")
		r.projectControls(nil, cv, comp)
		if cv.Turns[0].Editing == nil || cv.Turns[0].Editing.Text != "Edited draft" {
			t.Fatal("the edit was not projected")
		}
		delete(r.conversationEdits, cv.ID)
		r.projectConversation(cv, id, state, time.Unix(21, 0), "en")
		r.projectControls(nil, cv, comp)
		if cv.Turns[0].Editing != nil {
			t.Fatal("cancelled edit remained on a retained completed turn")
		}
		r.composerOutgoing[cv.ID] = &conversationOutgoing{transcript: true, input: conversation.Turn{ID: "outgoing/fixture", User: "Pending input"}}
		for range 2 {
			r.projectConversation(cv, id, state, time.Unix(21, 0), "en")
			r.projectOutgoing(cv, state.Snapshot.Page)
			if len(cv.Turns) != 2 || cv.Turns[1].User != "Pending input" {
				t.Fatal("an outgoing message disappeared or duplicated while history was retained")
			}
		}
		r.discardOutgoing(cv.ID)
		r.projectConversation(cv, id, state, time.Unix(22, 0), "en")
		if len(cv.Turns) != 1 || cv.Turns[0].User != "Latest input" {
			t.Fatal("discarding outgoing input kept its temporary history overlay")
		}
	})
}

func TestConversationActionReadsBeyondFrameCache(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source, cv, id := cacheTestRuntime(t)
		state := cacheTestFrame(t, r, cv, id)
		turn := state.Snapshot.Page.Runs[0].Turn
		comp := composer.New("")
		x := &conversationActions{r: r, cv: cv, comp: comp}
		if x.controls(turn).NoEdit {
			t.Fatal("the completed fixture cannot be edited")
		}
		changed := source.conversations[id]
		changed.Sequence++
		changed.Page.Runs = append([]protocol.ConversationRun{}, changed.Page.Runs...)
		run := protocol.ConversationRun{Session: id, Turn: protocol.TurnID{3}, Sequence: 3, Revision: 1,
			Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}
		changed.Page.Runs = append(changed.Page.Runs, run)
		source.conversations[id] = changed
		if err := r.controller.RefreshConversation(t.Context(), id); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		current, exists := r.controller.Conversation(id)
		if !exists || !pageBusy(current.Snapshot.Page) {
			t.Fatal("the Client did not receive the newly running turn")
		}
		// Model the interval after the Client changes but before its UI callback
		// is delivered. Rendering still owns its earlier accepted source copy.
		r.mu.Lock()
		clear(r.conversationChanges)
		r.mu.Unlock()
		r.beginConversationFrame()
		if _, cached, ok := x.viewState(); !ok || pageBusy(cached.Page) {
			t.Fatal("the fixture did not retain the earlier frame copy")
		}
		x.Edit(turn.String())
		if _, editing := r.conversationEdits[cv.ID]; editing {
			t.Fatal("an action used the cached idle guard after the Client became busy")
		}
	})
}

func TestRetainedToolDetailEnrichment(t *testing.T) {
	summary := &protocol.ToolDetailSummary{HasDetails: true}
	loaded := protocol.DefaultToolDetail()
	loaded.Resolved = &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{
		Preview: "Result", Paths: []string{"result.txt"}, Notices: []protocol.ToolResultLocalText{{Label: "Completed"}},
	}}
	call := conversation.Call{ID: "tool", Name: "fixture", Output: &conversation.Output{Images: []conversation.Image{{ID: "image"}}}}
	for range 3 {
		ProjectToolDetail(&call, summary, loaded, "en")
		if call.Output == nil || call.Output.Text != "Result" || len(call.Output.Paths) != 1 || len(call.Output.Notices) != 1 ||
			len(call.Output.Images) != 1 || call.Output.Images[0].ID != "image" {
			t.Fatalf("enriching a retained tool row accumulated or lost its output: %#v", call.Output)
		}
	}
}
