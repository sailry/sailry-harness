package client

import (
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
