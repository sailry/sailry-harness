package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCanonicalProjectionSuppressesOnlyLegacyDuplicateDiagnostics(t *testing.T) {
	const diagnostic = "Model stream ended before a completion event"
	for _, sample := range []struct {
		name    string
		failure bool
		blocks  []canonical.Block
		want    []protocol.ConversationPart
	}{
		{"legacy diagnostic", true, []canonical.Block{canonical.Text(0, diagnostic)}, nil},
		{"partial before diagnostic", true, []canonical.Block{canonical.Text(0, "Partial answer"), canonical.Text(1, diagnostic)}, []protocol.ConversationPart{{Kind: "text", Data: "Partial answer"}}},
		{"partial after diagnostic", true, []canonical.Block{canonical.Text(0, diagnostic), canonical.Text(1, "Partial answer")}, []protocol.ConversationPart{{Kind: "text", Data: "Partial answer"}}},
		{"ordinary matching text", false, []canonical.Block{canonical.Text(0, diagnostic)}, []protocol.ConversationPart{{Kind: "text", Data: diagnostic}}},
		{"near match", true, []canonical.Block{canonical.Text(0, diagnostic+"\n")}, []protocol.ConversationPart{{Kind: "text", Data: diagnostic + "\n"}}},
		{"matching reasoning", true, []canonical.Block{{Index: 0, Kind: "thinking", Data: &canonical.Thinking{Text: diagnostic}}}, []protocol.ConversationPart{{Kind: "thinking", Data: diagnostic}}},
	} {
		t.Run(sample.name, func(t *testing.T) {
			message := canonical.NewMessage(canonical.Assistant, sample.blocks...)
			if sample.failure {
				message.Error = &canonical.ResponseError{Message: diagnostic}
			}
			record := canonical.NewRecord(protocol.NewTurnID(), "message", &message, time.Unix(1, 0))
			record.Provenance.Author = "assistant"
			before, err := json.Marshal(record)
			if err != nil {
				t.Fatal(err)
			}
			entry, err := CanonicalEntry(record, 1)
			if err != nil {
				t.Fatal(err)
			}
			if len(entry.Parts) != len(sample.want) {
				t.Fatalf("projected parts=%+v, want %+v", entry.Parts, sample.want)
			}
			for index, want := range sample.want {
				if !reflect.DeepEqual(entry.Parts[index], want) {
					t.Fatalf("projected part=%+v, want %+v", entry.Parts[index], want)
				}
			}
			after, err := json.Marshal(record)
			if err != nil || !bytes.Equal(before, after) {
				t.Fatal("projection changed the canonical diagnostic or partial content")
			}
		})
	}
}

func TestStoredProjectionKeepsResourcePositionsAfterLegacyDiagnostic(t *testing.T) {
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	message := canonical.NewMessage(canonical.Assistant,
		canonical.Text(0, "Fixture diagnostic"),
		canonical.Block{Index: 1, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: strings.Repeat("a", 64), Mime: "image/png", Size: 1}}},
		canonical.Text(2, "Actual partial answer"),
	)
	message.Error = &canonical.ResponseError{Message: "Fixture diagnostic"}
	record := canonical.NewRecord(turn, "message", &message, now)
	record.Provenance.Author = "assistant"
	err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
		entry, err := projectStoredEntry(t.Context(), db, record, 1, nil)
		if err != nil {
			return err
		}
		if len(entry.Parts) != 2 || entry.Parts[0].Kind != "image" || entry.Parts[1].Kind != "text" || entry.Parts[1].Data != "Actual partial answer" {
			t.Fatalf("legacy diagnostic shifted or hid actual content: %+v", entry.Parts)
		}
		image, ok := entry.Parts[0].Data.(*protocol.ConversationImage)
		if !ok || image.Entry != record.ID || image.Part != 1 || image.Attachment.Spec.Worktree != session.Worktree {
			t.Fatalf("image lost its canonical identity after diagnostic suppression: %+v", image)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestHistoryPageMatchesOnlyRequestedCallsAtResponseSequence(t *testing.T) {
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		add := func(branch, id string) uint64 {
			m := canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(`{"path":"file"}`)}})
			r := canonical.NewRecord(turn, "message", &m, now)
			r.Provenance.Author = "assistant"
			r.Provenance.Branch = branch
			entry, _, err := AppendCanonical(ctx, db, session.ID, r)
			if err != nil {
				t.Fatal(err)
			}
			return entry.Sequence
		}
		add("other", "reused")
		add("main", "off-page")
		first := add("main", "reused")
		second := add("main", "reused")
		response := &canonical.ToolResponse{ID: "reused", Name: "read_file"}
		m := canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: response})
		r := canonical.NewRecord(turn, "message", &m, now)
		r.Provenance.Branch = "main"
		page := toolRequests{}
		page.include(r)
		for _, cache := range []toolRequests{page, nil} {
			for _, boundary := range []uint64{first, second} {
				tool, err := cache.find(ctx, db, r, boundary, response)
				if err != nil {
					return err
				}
				if (tool != nil) != (boundary == first) {
					t.Fatalf("incorrect match at sequence %d", boundary)
				}
			}
		}
		if _, ok := page[turn].matches[toolCallKey{"main", "off-page", "read_file"}]; ok {
			t.Fatal("retained an unrequested tool call")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
