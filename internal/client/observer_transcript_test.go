package client

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTranscriptRetriesUseModelRequestIdentity(t *testing.T) {
	for _, test := range []struct {
		name     string
		ids      []string
		fallback bool
		want     int
	}{
		{"one request", []string{"request-a", "request-a", "request-a"}, false, 1},
		{"different requests with increasing attempts", []string{"request-a", "request-b"}, false, 2},
		{"unknown request identity", []string{"", ""}, false, 2},
		{"fallback marker", []string{"request-a", "request-a"}, true, 2},
	} {
		t.Run(test.name, func(t *testing.T) {
			snapshot := conversationFixture()
			turn := snapshot.Page.Runs[0].Turn
			for index, id := range test.ids {
				raw, err := json.Marshal(map[string]any{"type": "model_retry", "id": id, "attempt": index + 2, "limit": 9, "fallback": test.fallback})
				if err != nil {
					t.Fatal(err)
				}
				snapshot.Page.Entries = append(snapshot.Page.Entries, protocol.ConversationEntry{ID: fmt.Sprint(index), Turn: turn, Author: "assistant",
					Parts: []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(raw)}},
				})
			}
			for _, referenceRows := range []bool{false, true} {
				rows := collectTranscriptRows(snapshot, nil, turn, referenceRows)
				if len(rows) != test.want {
					t.Fatalf("retry request identity changed its rows: %+v", rows)
				}
				last := rows[len(rows)-1]
				wantInline := !referenceRows || len(test.ids) != test.want
				if (last.Part != nil) != wantInline {
					t.Fatalf("referenceRows=%t: unexpected inline part: %+v", referenceRows, last)
				}
				part := last.Part
				if part == nil {
					part = &snapshot.Page.Entries[len(snapshot.Page.Entries)-1].Parts[0]
				}
				if attempt := resourceValue(part.Data).Attempt; attempt != uint64(len(test.ids)+1) {
					t.Fatalf("displayed retry attempt: %d", attempt)
				}
				if rows[0].Source.Entry != "0" || resourceValue(snapshot.Page.Entries[1].Parts[0].Data).Attempt != 2 {
					t.Fatal("retry update changed the row identity or canonical attempt")
				}
			}
		})
	}
}

func TestObserverTranscriptReferences(t *testing.T) {
	snapshot := conversationFixture()
	turn := snapshot.Page.Runs[0].Turn
	image := protocol.ConversationImage{Entry: "answer", Part: 8, Index: 2,
		Attachment: protocol.Attachment{ID: protocol.AttachmentID{1}, Spec: protocol.AttachmentSpec{
			Worktree: protocol.WorktreeID{1}, Name: "capture.png", MediaType: "image/png", Size: 1, Revision: "test",
		}},
	}
	direct, consecutive := image, image
	direct.Part, direct.Index = 5, 0
	consecutive.Part, consecutive.Index = 6, 0
	snapshot.Page.Entries = append(snapshot.Page.Entries, protocol.ConversationEntry{
		ID: "answer", Turn: turn, Sequence: 2, Author: "assistant",
		Parts: []protocol.ConversationPart{
			{Kind: "text", Data: " \n"},
			{Kind: "thinking", Data: "Working "}, {Kind: "thinking", Data: "🙂"},
			{Kind: "text", Data: new("A😀")}, {Kind: "text", Data: "中"},
			{Kind: "image", Data: direct}, {Kind: "image", Data: consecutive},
			{Kind: "text", Data: "B"},
			{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{
				Name: "capture", Summary: &protocol.ToolDetailSummary{Revision: "image-summary", Images: 1}, Images: []protocol.ConversationImage{image},
			}},
		},
	})
	snapshot.Drafts = []protocol.ConversationDraft{{ID: "draft", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Streaming draft"}}}}
	session := snapshot.Page.Session
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	controller := &Client{conversations: map[protocol.SessionID]*conversationWatch{
		session: {ctx: ctx, cancel: cancel, projection: &ConversationProjection{snapshot: &snapshot}, callbacks: map[uint64]func(){}},
	}}
	argument, _ := json.Marshal(session)
	legacy, err := controller.Watch(ctx, "watch_conversation", argument)
	if err != nil {
		t.Fatal(err)
	}
	defer legacy.Close()
	mobile, err := controller.WatchConversationView(ctx, session, ConversationViewOptions{ReferenceRows: true, Recent: true})
	if err != nil {
		t.Fatal(err)
	}
	defer mobile.Close()

	source := func(index uint64) toolReference { return toolReference{Entry: "answer", Index: index} }
	want := []transcriptRow{
		{Source: source(1), End: 3, TextOffset: 2, Work: true},
		{Source: source(3), End: 5, TextOffset: 2},
		{Source: source(5), End: 6, TextOffset: 5},
		{Source: source(6), End: 7, TextOffset: 5, Continuation: true},
		{Source: source(7), End: 8, TextOffset: 5, TextLast: true},
		{Source: source(8), End: 9, TextOffset: 6},
		{Source: source(8), End: 9, TextOffset: 6, Work: true},
		{Source: toolReference{Entry: "draft"}, Draft: true, End: 1, TextLast: true},
	}
	read := func(observer *Observer) (json.RawMessage, json.RawMessage, []transcriptRow) {
		t.Helper()
		encoded, err := observer.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		var packet struct {
			Snapshot json.RawMessage                `json:"snapshot"`
			Segments map[string][]transcriptSegment `json:"segments"`
		}
		if err := json.Unmarshal(encoded, &packet); err != nil {
			t.Fatal(err)
		}
		turnSegments := packet.Segments[turn.String()]
		if len(turnSegments) != 1 || len(turnSegments[0].Rows) != len(want) {
			t.Fatalf("unexpected transcript segments: %+v", turnSegments)
		}
		return encoded, packet.Snapshot, turnSegments[0].Rows
	}
	legacyJSON, legacySnapshot, legacyRows := read(legacy)
	mobileJSON, mobileSnapshot, mobileRows := read(mobile)
	if !bytes.Equal(legacySnapshot, mobileSnapshot) {
		t.Fatal("Mobile observation changed the raw snapshot")
	}
	for _, marker := range []string{"Working ", "A😀", "Streaming draft"} {
		if legacyCopies, mobileCopies := bytes.Count(legacyJSON, []byte(marker)), bytes.Count(mobileJSON, []byte(marker)); legacyCopies != 2 || mobileCopies != 1 {
			t.Errorf("%q occurs %d times on the legacy view and %d on Mobile; want 2 and 1", marker, legacyCopies, mobileCopies)
		}
	}
	wantKinds := []string{"thinking", "text", "image", "image", "text", "image", "tool_result", "text"}
	for i, kind := range wantKinds {
		if part := legacyRows[i].Part; part == nil || part.Kind != kind {
			t.Fatalf("legacy row %d lost its inline %s: %+v", i, kind, part)
		}
	}
	for i, text := range map[int]string{0: "Working 🙂", 1: "A😀中", 4: "B", 7: "Streaming draft"} {
		if got, err := payload[string](legacyRows[i].Part.Data); err != nil || got != text {
			t.Fatalf("legacy row %d text: %q (%v); want %q", i, got, err, text)
		}
	}
	for _, rows := range [][]transcriptRow{legacyRows, mobileRows} {
		override := rows[5].Part
		if override == nil || override.Kind != "image" {
			t.Fatalf("tool result lost its image override: %+v", override)
		}
		if got, err := payload[protocol.ConversationImage](override.Data); err != nil || got != image {
			t.Fatalf("tool result changed its image override: %+v (%v)", got, err)
		}
	}
	for i := range legacyRows {
		legacyRows[i].Part = nil
	}
	mobileRows[5].Part = nil
	if !slices.Equal(legacyRows, want) || !slices.Equal(mobileRows, want) {
		t.Fatalf("unexpected row metadata: legacy=%+v, Mobile=%+v; want %+v", legacyRows, mobileRows, want)
	}
}

func BenchmarkObserverTranscriptJSON(b *testing.B) {
	text := strings.Repeat("Synthetic reply 😀\n", 64)
	thinking := strings.Repeat("Synthetic reasoning\n", 64)
	parts := []protocol.ConversationPart{
		{Kind: "thinking", Data: thinking}, {Kind: "thinking", Data: thinking},
		{Kind: "text", Data: text}, {Kind: "text", Data: text},
	}
	for _, turns := range []int{8, 128} {
		b.Run(fmt.Sprintf("%d_turns", turns), func(b *testing.B) {
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Node, snapshot.Page.Session = protocol.NodeID{1}, protocol.SessionID{1}
			for i := range turns {
				turn := protocol.TurnID{byte(i + 1)}
				snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{
					Session: snapshot.Page.Session, Turn: turn, Sequence: uint64(i + 1), Kind: "task", Status: "completed",
				})
				snapshot.Page.Entries = append(snapshot.Page.Entries,
					protocol.ConversationEntry{ID: fmt.Sprintf("input/%d", i), Turn: turn, Sequence: uint64(2*i + 1), Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Request"}}},
					protocol.ConversationEntry{ID: fmt.Sprintf("answer/%d", i), Turn: turn, Sequence: uint64(2*i + 2), Author: "assistant", Parts: parts},
				)
			}
			last := &snapshot.Page.Runs[turns-1]
			last.Status = "running"
			snapshot.Drafts = []protocol.ConversationDraft{{ID: "draft", Turn: last.Turn, Author: "assistant", Parts: parts}}
			for _, mode := range []struct {
				name          string
				referenceRows bool
			}{{"legacy", false}, {"mobile", true}} {
				b.Run(mode.name, func(b *testing.B) {
					b.ReportAllocs()
					var body []byte
					var err error
					for b.Loop() {
						timeline, owners, segments := projectTranscript(snapshot, nil, nil, mode.referenceRows)
						body, err = json.Marshal(map[string]any{"snapshot": snapshot, "segments": segments, "timeline": timeline, "owners": owners})
						if err != nil {
							b.Fatal(err)
						}
					}
					b.ReportMetric(float64(len(body)), "json-B/op")
				})
			}
		})
	}
}

func TestObserverToolDetailContentPaths(t *testing.T) {
	for _, root := range []string{`{"sailry_content":[],"a/b":["first","second"]}`, `{"structuredContent":{"sailry_content":[],"a/b":["first","second"]}}`, `{"output":{"a/b":["first","second"]}}`} {
		result := json.RawMessage(root)
		detail := protocol.DefaultToolDetail()
		detail.Result = &result
		detail.Content = &protocol.ToolContent{}
		for _, path := range []string{"/a~1b/1", "/a~1b/0", "/missing"} {
			detail.Content.Blocks = append(detail.Content.Blocks, protocol.ToolContentBlock{Kind: "text", Data: protocol.ToolContentText{Path: path}})
		}
		packet := toolDetailPacket(detail, ToolCall{})
		texts := packet["texts"].([]any)
		if len(texts) != 3 || texts[0].(*string) == nil || *texts[0].(*string) != "second" || texts[1].(*string) == nil || *texts[1].(*string) != "first" || texts[2].(*string) != nil {
			t.Fatalf("unexpected content paths for %s: %v", root, texts)
		}
	}
}

func TestObserverToolFailureSeparatesMessageAndDetail(t *testing.T) {
	const path = "src/review.go"
	const message = "The reviewed path is missing"
	summary := &protocol.ToolDetailSummary{
		HasDetails: true,
		Path:       new(path),
		Error:      new(message),
		Failure:    &protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool},
	}
	call := ToolCall{State: "returned", Summary: summary}
	value := projectToolCall(protocol.ConversationPage{}, call)
	row := value["row"].(map[string]any)
	detail, _ := row["detail"].(*protocol.ToolDetailText)
	if detail == nil || detail.Text != path || value["error"] != message || value["error_detail"] != nil {
		t.Fatalf("known tool failure changed its input or requested the original result: %+v", value)
	}

	summary.Error = nil
	value = projectToolCall(protocol.ConversationPage{}, call)
	if value["error"] != nil || value["error_detail"] != true {
		t.Fatalf("unexplained tool failure did not expose the original result: %+v", value)
	}
	for _, code := range []protocol.ErrorCode{
		protocol.ErrorCodeCancelled, protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodePermissionDenied,
	} {
		summary.Failure.Code = &code
		if toolNeedsErrorDetail(call) {
			t.Errorf("%s requested an original result", code)
		}
	}
	summary.Failure.Code = new(protocol.ErrorCodeNotFound)
	summary.Expected = new(protocol.ToolResultExpectedFileNotFound)
	if toolNeedsErrorDetail(call) {
		t.Fatal("an expected result requested details")
	}
	summary.Failure.Code, summary.Expected = nil, nil
	call.Approval = &protocol.ConversationApproval{State: "denied"}
	if toolNeedsErrorDetail(call) {
		t.Fatal("a denied call requested details")
	}
}
