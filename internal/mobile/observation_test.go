package mobile

import (
	"context"
	"encoding/json"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type historyTransport struct {
	noticeTransport
	snapshots  chan protocol.Update
	reads      chan protocol.CommandReadConversationPayload
	completion chan link.Response
}

func (transport historyTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "conversation" {
		return nil, unavailable("unexpected observation topic")
	}
	return topicStream{updates: transport.snapshots}, nil
}

func (transport historyTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	query, ok := request.Command.Data.(protocol.CommandReadConversationPayload)
	if !ok {
		return link.Admission{}, unavailable("unexpected history command")
	}
	select {
	case transport.reads <- query:
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: transport.completion}, nil
	case <-ctx.Done():
		return link.Admission{}, ctx.Err()
	}
}

func TestMobileConversationHistoryCompletion(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	node, session := protocol.NodeID{1}, protocol.SessionID{1}
	large := strings.Repeat("Unicode 世界, \"quotes\", \\ and \n", 2048)
	page := func(turn protocol.TurnID, sequence uint64, text string) protocol.ConversationPage {
		page := protocol.DefaultConversationPage()
		page.Session, page.Revision = session, 1
		page.Runs = []protocol.ConversationRun{{Turn: turn, Sequence: sequence, Session: session, Worktree: protocol.WorktreeID{1}, Status: "completed", Kind: "task", Revision: 1}}
		page.Entries = []protocol.ConversationEntry{{Turn: turn, ID: turn.String(), Sequence: sequence, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}}}}
		return page
	}
	older, latest := protocol.TurnID{1}, protocol.TurnID{2}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence, snapshot.Page = node, 2, page(latest, 2, "latest")
	snapshot.Page.NextBefore = &latest
	transport := historyTransport{
		noticeTransport: noticeTransport{node: node},
		snapshots:       make(chan protocol.Update, 1),
		reads:           make(chan protocol.CommandReadConversationPayload, 1),
		completion:      make(chan link.Response, 1),
	}
	type completion struct {
		request uint64
		body    []byte
	}
	completed := make(chan completion, 4)
	runtime := New(func(request uint64, data []byte) { completed <- completion{request, data} })
	defer runtime.Close(0)
	shared := client.New(transport)
	lifetime, stop := context.WithCancel(runtime.ctx)
	if _, err := runtime.add(0, "connection", &connection{client: shared}, lifetime, stop); err != nil {
		t.Fatal(err)
	}
	var request uint64
	send := func(handle uint64, method, input string) uint64 {
		t.Helper()
		request++
		if !runtime.Invoke(handle, method, []byte(input), request) {
			t.Fatalf("%s was not accepted", method)
		}
		return request
	}
	receive := func() completion {
		t.Helper()
		select {
		case value := <-completed:
			return value
		case <-ctx.Done():
			t.Fatal("missing native completion")
			return completion{}
		}
	}
	decode := func(data []byte) envelope {
		t.Helper()
		var value envelope
		if err := json.Unmarshal(data, &value); err != nil {
			t.Fatalf("decode legacy envelope: %v", err)
		}
		return value
	}
	type view struct {
		Snapshot     *protocol.ConversationSnapshot `json:"snapshot"`
		LoadingOlder bool                           `json:"loading_older"`
	}
	decodeView := func(data []byte) view {
		t.Helper()
		var value view
		if err := json.Unmarshal(data, &value); err != nil {
			t.Fatalf("decode conversation view: %v", err)
		}
		return value
	}
	send(1, "watch_conversation", `{"session":"`+session.String()+`","recent":true,"reference_rows":true}`)
	opened := decode(receive().body)
	value, ok := opened.Value.(map[string]any)
	if !opened.OK || !ok || value["kind"] != "conversation_updates" || value["handle"] != "2" {
		t.Fatalf("watch completion = %#v", opened)
	}
	send(2, "next", `{}`)
	initial := decode(receive().body)
	text, ok := initial.Value.(string)
	if !initial.OK || !ok || decodeView([]byte(text)).Snapshot != nil {
		t.Fatalf("initial observation before the remote snapshot = %#v", initial)
	}
	transport.snapshots <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}

	const raw = `{"raw_view":true}`
	decodeRaw := func(data []byte) view {
		t.Helper()
		if len(data) == 0 || data[0] != 0 {
			t.Fatal("opted-in observation did not carry raw JSON")
		}
		return decodeView(data[1:])
	}
	for {
		send(2, "next", raw)
		if first := decodeRaw(receive().body).Snapshot; first != nil {
			if first.Page.NextBefore == nil || *first.Page.NextBefore != latest || len(first.Page.Runs) != 1 || first.Page.Runs[0].Turn != latest {
				t.Fatalf("unexpected initial page: %#v", first.Page)
			}
			break
		}
	}

	next := send(2, "next", raw)
	load := send(2, "load_older", `{}`)
	var observedLoading, queued bool
	for !observedLoading || !queued {
		callback := receive()
		switch callback.request {
		case next:
			observedLoading = decodeRaw(callback.body).LoadingOlder
			if !observedLoading {
				next = send(2, "next", raw)
			}
		case load:
			if ack := decode(callback.body); !ack.OK {
				t.Fatalf("queue history = %#v", ack)
			}
			queued = true
		default:
			t.Fatalf("unexpected completion %d", callback.request)
		}
	}
	select {
	case query := <-transport.reads:
		if query.Before == nil || *query.Before != latest || query.Session != session {
			t.Fatalf("history query = %#v", query)
		}
	case <-ctx.Done():
		t.Fatal("history request was not sent")
	}
	transport.completion <- link.Response{Output: protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{
		Sequence: snapshot.Sequence, Page: page(older, 1, large), Missing: []protocol.TurnID{},
	}}}
	send(2, "next", raw)
	packet := receive().body
	if len(packet) < 32*1024 {
		t.Fatal("merged history did not arrive as a large raw completion")
	}
	merged := decodeRaw(packet).Snapshot
	if merged == nil || merged.Page.NextBefore != nil || len(merged.Page.Entries) != 2 ||
		merged.Page.Entries[0].Turn != older || merged.Page.Entries[1].Turn != latest {
		t.Fatal("history cursor or order changed in the raw completion")
	}
	parts := merged.Page.Entries[0].Parts
	if len(parts) != 1 || parts[0].Kind != "text" {
		t.Fatal("the older text part is missing")
	}
	if text, ok := parts[0].Data.(*string); !ok || *text != large {
		t.Fatal("the older text changed in the raw completion")
	}
	send(2, "close", `{}`)
	if value := decode(receive().body); !value.OK {
		t.Fatalf("observation close = %#v", value)
	}
	send(2, "next", raw)
	if value := decode(receive().body); value.OK || value.Error == nil || value.Error.Code != protocol.ErrorCodeUnavailable {
		t.Fatalf("closed observation = %#v", value)
	}
	runtime.Close(request + 1)
	if final := decode(receive().body); !final.Closed || final.Count != strconv.FormatUint(request+1, 10) {
		t.Fatalf("bridge shutdown = %#v", final)
	}
}
