package client

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReadToolDetailWithoutExpandableCard(t *testing.T) {
	t.Parallel()
	for _, observed := range []bool{false, true} {
		name := "unobserved"
		if observed {
			name = "pending_without_details"
		}
		t.Run(name, func(t *testing.T) {
			ref := toolDetailReferenceFixture()
			detail := protocol.DefaultToolDetail()
			detail.Arguments = json.RawMessage(`{"command":"printf acceptance","background":false}`)
			encoded, err := json.Marshal(detail)
			if err != nil {
				t.Fatal(err)
			}
			const pageSize = 17
			reads := 0
			transport := toolDetailTransportFixture(t, ref, func(query protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage {
				reads++
				offset := uint64(0)
				if query.Cursor != nil {
					offset = *query.Cursor
				}
				if offset >= uint64(len(encoded)) || offset%pageSize != 0 {
					t.Fatalf("invalid continuation cursor: %v", query.Cursor)
				}
				end := min(offset+pageSize, uint64(len(encoded)))
				page := protocol.ToolDetailPage{Reference: ref, Offset: offset, TotalBytes: uint64(len(encoded)), Text: string(encoded[offset:end])}
				if end < page.TotalBytes {
					page.NextOffset = &end
				}
				return page
			})
			controller := New(transport)
			defer controller.Close()
			if observed {
				summary := protocol.DefaultToolDetailSummary()
				summary.Revision = ref.Revision
				call := protocol.DefaultConversationPartToolCallPayload()
				call.ID, call.Name, call.Summary = ref.Call, "run_command", &summary
				snapshot := protocol.DefaultConversationSnapshot()
				snapshot.Page.Session = ref.Session
				snapshot.Page.Entries = []protocol.ConversationEntry{{ID: ref.Entry, Turn: ref.Turn, Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}}}
				projection := NewConversationProjection(controller.Target(), ref.Session, 1)
				projection.snapshot = &snapshot
				controller.conversations = map[protocol.SessionID]*conversationWatch{ref.Session: {projection: projection}}
				defer func() { controller.conversations = nil }()
			}
			got, err := controller.ReadToolDetail(t.Context(), ref)
			if err != nil || !reflect.DeepEqual(got, detail) || reads != (len(encoded)+pageSize-1)/pageSize {
				t.Fatalf("original pending detail = %+v, pages %d, error %v", got, reads, err)
			}
			if len(controller.conversationDetails) != 0 || len(controller.conversationDetailOrder) != 0 || controller.conversationDetailBytes != 0 {
				t.Fatal("a direct tool read populated the presentation cache")
			}
			if _, err := controller.ReadConversationDetail(t.Context(), ref); faultCode(err) != protocol.ErrorCodeRevisionConflict {
				t.Fatalf("an unretained or nonexpandable card entered the presentation cache: %v", err)
			}
		})
	}
}

func TestReadToolDetailStopsAfterCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	ref := toolDetailReferenceFixture()
	reads := 0
	transport := toolDetailTransportFixture(t, ref, func(protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage {
		reads++
		cancel()
		return protocol.ToolDetailPage{Reference: ref, TotalBytes: 40, Text: "{", NextOffset: new(uint64(1))}
	})
	controller := New(transport)
	defer controller.Close()
	if _, err := controller.ReadToolDetail(ctx, ref); err == nil || reads != 1 {
		t.Fatalf("cancelled transfer made %d reads, error %v", reads, err)
	}
}

func TestReadToolDetailRejectsInvalidPages(t *testing.T) {
	t.Parallel()
	ref := toolDetailReferenceFixture()
	encoded, err := json.Marshal(protocol.DefaultToolDetail())
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		name   string
		change func(*protocol.ToolDetailPage)
		want   string
	}{
		{"session", func(page *protocol.ToolDetailPage) { page.Reference.Session = protocol.SessionID{9} }, "inconsistent"},
		{"turn", func(page *protocol.ToolDetailPage) { page.Reference.Turn = protocol.TurnID{9} }, "inconsistent"},
		{"entry", func(page *protocol.ToolDetailPage) { page.Reference.Entry = "other" }, "inconsistent"},
		{"index", func(page *protocol.ToolDetailPage) { page.Reference.Index++ }, "inconsistent"},
		{"call", func(page *protocol.ToolDetailPage) { page.Reference.Call = new("other-call") }, "inconsistent"},
		{"revision", func(page *protocol.ToolDetailPage) { page.Reference.Revision = strings.Repeat("b", 64) }, "inconsistent"},
		{"transfer_limit", func(page *protocol.ToolDetailPage) { page.TotalBytes = conversationDetailLimit + 1 }, detailTransferLimit},
		{"page_limit", func(page *protocol.ToolDetailPage) {
			page.Text = strings.Repeat("x", 64*1024+1)
			page.TotalBytes = uint64(len(page.Text))
		}, "inconsistent"},
		{"truncated", func(page *protocol.ToolDetailPage) { page.TotalBytes++ }, "inconsistent"},
	} {
		t.Run(test.name, func(t *testing.T) {
			reads := 0
			transport := toolDetailTransportFixture(t, ref, func(protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage {
				reads++
				page := protocol.ToolDetailPage{Reference: ref, TotalBytes: uint64(len(encoded)), Text: string(encoded)}
				test.change(&page)
				return page
			})
			controller := New(transport)
			defer controller.Close()
			_, err := controller.ReadToolDetail(t.Context(), ref)
			if faultCode(err) != protocol.ErrorCodeInvalidRequest || !strings.Contains(err.Error(), test.want) || reads != 1 {
				t.Fatalf("invalid detail accepted or continued reading: reads %d, error %v", reads, err)
			}
		})
	}
}

func TestReadConversationDetailNotifiesEvictedConversations(t *testing.T) {
	t.Parallel()
	ref := toolDetailReferenceFixture()
	detail := protocol.DefaultToolDetail()
	encoded, err := json.Marshal(detail)
	if err != nil {
		t.Fatal(err)
	}
	transport := toolDetailTransportFixture(t, ref, func(protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage {
		return protocol.ToolDetailPage{Reference: ref, TotalBytes: uint64(len(encoded)), Text: string(encoded)}
	})
	controller := New(transport)
	defer controller.Close()
	controller.conversations = map[protocol.SessionID]*conversationWatch{}
	defer func() { controller.conversations = nil }()
	controller.conversationDetails = map[string]conversationDetail{}

	other, another := protocol.SessionID{3}, protocol.SessionID{4}
	retained := ref
	retained.Session, retained.Entry = protocol.SessionID{5}, "retained"
	changes := map[protocol.SessionID]int{}
	// Simulate a full byte budget without allocating large detail payloads.
	for _, fixture := range []struct {
		session protocol.SessionID
		entry   string
		bytes   uint64
	}{
		{other, "old-1", 1},
		{other, "old-2", 1},
		{ref.Session, "old-3", 1},
		{another, "old-4", conversationDetailLimit / 2},
		{retained.Session, retained.Entry, conversationDetailLimit/2 - 3},
		{ref.Session, ref.Entry, 0},
	} {
		card := ref
		card.Session, card.Entry = fixture.session, fixture.entry
		w := controller.conversations[card.Session]
		if w == nil {
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Page.Session = card.Session
			projection := NewConversationProjection(controller.Target(), card.Session, 1)
			projection.snapshot = &snapshot
			w = &conversationWatch{
				session:    card.Session,
				projection: projection,
				signal:     make(chan struct{}),
				callbacks: map[uint64]func(){1: func() {
					changes[card.Session]++
					if _, ok := controller.ConversationDetail(ref); !ok {
						t.Error("conversation changed before the new detail was cached")
					}
				}},
			}
			controller.conversations[card.Session] = w
		}
		summary := protocol.DefaultToolDetailSummary()
		summary.Revision, summary.HasDetails = card.Revision, true
		call := protocol.DefaultConversationPartToolCallPayload()
		call.ID, call.Name, call.Summary = card.Call, "run_command", &summary
		page := &w.projection.snapshot.Page
		page.Entries = append(page.Entries, protocol.ConversationEntry{ID: card.Entry, Turn: card.Turn, Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}})
		if fixture.bytes != 0 {
			key := detailKey(card)
			controller.conversationDetailOrder = append(controller.conversationDetailOrder, key)
			controller.conversationDetails[key] = conversationDetail{card, detail, fixture.bytes}
			controller.conversationDetailBytes += fixture.bytes
		}
	}
	if _, err := controller.ReadConversationDetail(t.Context(), ref); err != nil {
		t.Fatal(err)
	}
	if _, ok := controller.ConversationDetail(retained); !ok || len(controller.conversationDetails) != 2 {
		t.Fatalf("expected the retained and newly read details, cache size %d, retained %t", len(controller.conversationDetails), ok)
	}
	want := map[protocol.SessionID]int{ref.Session: 1, other: 1, another: 1}
	if !reflect.DeepEqual(changes, want) {
		t.Fatalf("conversation callbacks = %v, want %v", changes, want)
	}
}

func TestReadConversationDetailSurvivesFirstWaiterCancel(t *testing.T) {
	t.Parallel()
	ref := toolDetailReferenceFixture()
	detail := protocol.DefaultToolDetail()
	detail.Arguments = json.RawMessage(`{"command":"echo ok"}`)
	encoded, err := json.Marshal(detail)
	if err != nil {
		t.Fatal(err)
	}
	started := make(chan struct{})
	release := make(chan struct{})
	transport := toolDetailTransportFixture(t, ref, func(protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage {
		close(started)
		<-release
		return protocol.ToolDetailPage{Reference: ref, TotalBytes: uint64(len(encoded)), Text: string(encoded)}
	})
	controller := New(transport)
	defer controller.Close()
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision, summary.HasDetails = ref.Revision, true
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Summary = ref.Call, "run_command", &summary
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Page.Session = ref.Session
	snapshot.Page.Entries = []protocol.ConversationEntry{{ID: ref.Entry, Turn: ref.Turn, Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}}}
	projection := NewConversationProjection(controller.Target(), ref.Session, 1)
	projection.snapshot = &snapshot
	controller.conversations = map[protocol.SessionID]*conversationWatch{ref.Session: {projection: projection}}
	defer func() { controller.conversations = nil }()

	ctx1, cancel1 := context.WithCancel(t.Context())
	defer cancel1()
	type outcome struct {
		detail protocol.ToolDetail
		err    error
	}
	first := make(chan outcome, 1)
	second := make(chan outcome, 1)
	go func() {
		v, e := controller.ReadConversationDetail(ctx1, ref)
		first <- outcome{v, e}
	}()
	<-started
	go func() {
		v, e := controller.ReadConversationDetail(t.Context(), ref)
		second <- outcome{v, e}
	}()
	for {
		controller.conversationMu.Lock()
		pending := controller.conversationDetailReads[detailKey(ref)]
		ready := pending != nil && pending.waiters == 2
		controller.conversationMu.Unlock()
		if ready {
			break
		}
	}
	cancel1()
	if res := <-first; res.err == nil {
		t.Fatal("first waiter did not cancel")
	}
	close(release)
	if res := <-second; res.err != nil || !reflect.DeepEqual(res.detail, detail) {
		t.Fatalf("second waiter poisoned by first cancel: %+v, %v", res.detail, res.err)
	}
}

func toolDetailReferenceFixture() protocol.ToolDetailReference {
	return protocol.ToolDetailReference{Session: protocol.SessionID{1}, Turn: protocol.TurnID{2}, Entry: "pending-tool", Call: new("pending-call"), Revision: strings.Repeat("a", 64)}
}

func toolDetailTransportFixture(t *testing.T, ref protocol.ToolDetailReference, read func(protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage) *clientTransport {
	t.Helper()
	return &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
		query, err := payload[protocol.CommandReadToolDetailPayload](request.Command.Data)
		if err != nil || request.Command.Kind != "read_tool_detail" || request.Command.Durable() || query.Limit != 64*1024 {
			t.Fatalf("unexpected tool read: %+v, %v", request, err)
		}
		got := protocol.ToolDetailReference{Session: query.Session, Turn: query.Turn, Entry: query.Entry, Index: query.Index, Call: query.Call, Revision: query.ExpectedRevision}
		if !reflect.DeepEqual(got, ref) {
			t.Fatalf("tool read lost its captured scope: %+v", got)
		}
		completion := make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{Kind: "tool_detail", Data: read(query)}}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}}
}
