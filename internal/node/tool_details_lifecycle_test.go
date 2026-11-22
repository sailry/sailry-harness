package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestLargePublishedToolDetailThroughLocalAPI(t *testing.T) {
	owner := nodeFixture(t)
	session := sessionFixture(t, owner)
	queued := commandFixture(t, owner, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Publication fixture"},
	}}).Data.(protocol.QueuedTurn)
	ctx := t.Context()
	const call, name = "large-result", "present_files"
	const file = "output/chart.png"
	text := strings.Repeat("界", 64*1024)
	content := protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{
		{Kind: "text", Data: protocol.ToolContentText{Path: "/text"}},
		{Kind: "file", Data: protocol.ToolContentFile{Path: file, Mime: "image/png", Revision: new(strings.Repeat("a", 64)), Size: new(uint64(512))}},
	}}
	raw, err := json.Marshal(map[string]any{"text": text, "sailry_content": content})
	if err != nil {
		t.Fatal(err)
	}
	var object any
	if err := json.Unmarshal(raw, &object); err != nil {
		t.Fatal(err)
	}
	raw, err = json.Marshal(object)
	if err != nil {
		t.Fatal(err)
	}
	var entryID string
	if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		now := time.Now()
		start := protocol.NewRequest(owner.ID(), protocol.Command{Kind: "start_queued_turn", Data: &protocol.CommandStartQueuedTurnPayload{Turn: queued.ID}})
		if _, _, err := store.ExecuteAgentCommand(ctx, db, owner.ID(), owner.profile.Ownership.Path, owner.ID(), start, now); err != nil {
			return err
		}
		invocation, _, err := store.ClaimNext(ctx, db, now)
		if err != nil {
			return err
		}
		if invocation == nil || invocation.Run.Turn != queued.ID {
			return fmt.Errorf("fixture turn was not claimed")
		}
		response := &canonical.ToolResponse{ID: call, Name: name, Status: canonical.ToolSuccess,
			Revision: fmt.Sprintf("%x", blake3.Sum256(raw)), Result: canonical.Payload{Value: raw}}
		record := canonical.NewRecord(queued.ID, "message", canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: response}), now)
		record.Provenance.Author = "tool"
		entry, _, err := store.AppendCanonical(ctx, db, session.ID, record)
		if err != nil {
			return err
		}
		entryID = entry.ID
		_, _, err = store.FinishTurn(ctx, db, queued.ID, protocol.ConversationStatusCompleted, nil, now)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	transport := &countToolDetailPages{Transport: link.NewLocal(owner.ID(), owner.ID(), owner)}
	c := client.New(transport)
	defer c.Close()
	history, err := c.Do(ctx, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session.ID, Limit: 20}})
	if err != nil {
		t.Fatal(err)
	}
	page := history.Data.(protocol.ConversationHistory).Page
	var ref protocol.ToolDetailReference
	for _, entry := range page.Entries {
		if entry.ID != entryID {
			continue
		}
		result, ok := entry.Parts[0].Data.(*protocol.ConversationPartToolResultPayload)
		if !ok || result.Summary == nil {
			t.Fatalf("expected a projected tool result, got %T", entry.Parts[0].Data)
		}
		encoded, err := json.Marshal(result.Summary)
		if err != nil {
			t.Fatal(err)
		}
		var decoded protocol.ToolDetailSummary
		if err := protocol.DecodeAs(protocol.PeerData, encoded, &decoded); err != nil {
			t.Fatal(err)
		}
		if !decoded.HasDetails || !decoded.Published || len(decoded.Publications) != 1 || decoded.Publications[0].Index != 1 || len(encoded) > 2048 || bytes.Contains(encoded, []byte(strings.Repeat("界", 512))) {
			t.Fatalf("history did not supply a small publication summary: %d bytes, %+v", len(encoded), decoded.Publications)
		}
		asset := decoded.Publications[0].Block.Data.(*protocol.ToolContentFile)
		if asset.Path != file || asset.Mime != "image/png" || asset.Revision == nil || *asset.Revision != strings.Repeat("a", 64) {
			t.Fatalf("publication reference changed: %+v", asset)
		}
		ref = protocol.ToolDetailReference{Session: session.ID, Turn: queued.ID, Entry: entry.ID, Call: result.ID, Revision: decoded.Revision}
		t.Logf("history tool summary: %d B for %d B original JSON", len(encoded), len(raw))
	}
	if ref.Revision == "" || transport.pages != 0 {
		t.Fatalf("history missing result or fetched details: revision=%q pages=%d", ref.Revision, transport.pages)
	}
	loaded, err := c.ReadToolDetail(ctx, ref)
	if err != nil {
		t.Fatal(err)
	}
	if loaded.Result == nil || !bytes.Equal(*loaded.Result, raw) || transport.pages < 3 || !transport.validUTF8 {
		t.Fatalf("normal detail API lost the original JSON or Unicode paging: %d pages", transport.pages)
	}
	if _, cached := c.ConversationDetail(ref); cached {
		t.Fatal("plain tool detail read populated the conversation cache")
	}
	t.Logf("original JSON reconstructed across %d pages", transport.pages)

	if err := c.Start(ctx); err != nil {
		t.Fatal(err)
	}
	ready := make(chan struct{}, 1)
	release, err := c.WatchRecentConversation(ctx, session.ID, func() {
		if _, ok := c.LatestLocalTurnStatus(session.ID, queued.ID); ok {
			select {
			case ready <- struct{}{}:
			default:
			}
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	select {
	case <-ready:
	case <-time.After(5 * time.Second):
		t.Fatal("Client did not observe the canonical tool result")
	}
	if _, err := c.ReadConversationDetail(ctx, ref); err != nil {
		t.Fatal(err)
	}
	if cached, ok := c.ConversationDetail(ref); !ok || cached.Result == nil || !bytes.Equal(*cached.Result, raw) {
		t.Fatal("the explicit conversation detail read did not cache the original")
	}
	release()
	if _, cached := c.ConversationDetail(ref); cached {
		t.Fatal("last conversation release kept the original detail")
	}
}

type countToolDetailPages struct {
	link.Transport
	pages     int
	validUTF8 bool
}

func (t *countToolDetailPages) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	admission, err := t.Transport.Dispatch(ctx, request)
	if err != nil || request.Command.Kind != "read_tool_detail" {
		return admission, err
	}
	response, ok := <-admission.Completion
	forward := make(chan link.Response, 1)
	if ok {
		if page, ok := response.Output.Data.(protocol.ToolDetailPage); ok {
			t.validUTF8 = (t.pages == 0 || t.validUTF8) && utf8.ValidString(page.Text)
			t.pages++
		}
		forward <- response
	}
	close(forward)
	admission.Completion = forward
	return admission, nil
}
