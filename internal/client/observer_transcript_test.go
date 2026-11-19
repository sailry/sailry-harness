package client

import (
	"encoding/json"
	"fmt"
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
			rows := collectTranscriptRows(snapshot, nil, turn)
			if len(rows) != test.want || resourceValue(rows[len(rows)-1].Part.Data).Attempt != uint64(len(test.ids)+1) {
				t.Fatalf("retry request identity changed its rows: %+v", rows)
			}
			if rows[0].Source.Entry != "0" || resourceValue(snapshot.Page.Entries[1].Parts[0].Data).Attempt != 2 {
				t.Fatal("retry update changed the row identity or canonical attempt")
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
