package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestConversationAssetInventory(t *testing.T) {
	t.Parallel()
	t.Run("captured cross-platform paths", func(t *testing.T) {
		for _, root := range []*string{nil, ptr("/remote/project"), ptr(`C:\remote\project`), ptr(`\\?\C:\remote\project`), ptr(`\\?\UNC\server\share\project`)} {
			for _, entry := range [][2]string{{"report.md", "report.md"}, {"./report.md", "report.md"}, {"output/report%20one.pdf", "output/report one.pdf"}, {"output/%E8%B5%84%E6%96%99.pdf", "output/资料.pdf"}, {"output/a%2Bb.pdf", "output/a+b.pdf"}} {
				if path, ok := conversationAssetPath(entry[0], root); !ok || path != entry[1] {
					t.Fatalf("captured root %v lost %q: %q, %v", root, entry[0], path, ok)
				}
			}
			for _, link := range []string{"", "#section", "../secret", "%2e%2e/secret", "%2e%2e%2fsecret", "report%00.md", "report%0A.md", "https://example.test/report.md"} {
				if path, ok := conversationAssetPath(link, root); ok {
					t.Fatalf("escaped or invalid link %q resolved under %v: %q", link, root, path)
				}
			}
		}
		for _, entry := range [][3]string{
			{"/remote/project", "/remote/project/report.md", "report.md"},
			{"/remote/project", "file:///remote/project/report.md", "report.md"},
			{`\\?\C:\remote\project`, `c:\remote\project\report.md`, "report.md"},
			{`c:\remote\project`, "file:///C:/remote/project/report.md", "report.md"},
			{`\\?\UNC\server\share\project`, `\\server\share\project\report.md`, "report.md"},
			{"/remote/project", "/remote/other/report.md", ""},
			{"/remote/project", "file://other/remote/project/report.md", ""},
			{`C:\remote\project`, `D:\remote\project\report.md`, ""},
		} {
			if got, ok := conversationAssetPath(entry[1], &entry[0]); got != entry[2] || ok != (entry[2] != "") {
				t.Fatalf("absolute link %q under %q: %q, %v", entry[1], entry[0], got, ok)
			}
		}
	})
	t.Run("deliveries and navigation", func(t *testing.T) {
		for _, entry := range []struct {
			input bool
			text  string
			want  []string
		}{
			{false, "[Report](report.md)\n\n![Image](image.png)\n\n[Again](./report.md)\n\nSource: [Code](src/main.rs#L3) `![Code](code.png)` [Outside](../secret) [Web](https://example.test/a.png) plain.txt", []string{"report.md", "image.png"}},
			{false, "[Line](src/main.rs:42)\n\n[Section](guide.md#intro)\n\n[One](a.json) [Two](b.json)\n\nSee [Report](report.pdf)\n\n> [Source](src/quoted.rs)\n\n> > [Nested](src/nested.rs)\n\n`[Code](code.py)`\n\n[Web](https://example.test) [Mixed](mixed.json)\n\n- [Report](report.pdf)\n- [Data](data.json)", []string{"report.pdf", "data.json"}},
			{true, "See [Input](input.txt)\n\n> [Quoted](quoted.txt)", []string{"input.txt", "quoted.txt"}},
		} {
			targets := &conversationAssetTargets{}
			conversationAssetMarkdown(targets, entry.text, ptr("/project"), entry.input)
			got := []string{}
			for _, item := range targets.items {
				got = append(got, item.Data.(*protocol.ConversationAssetsTargetFilePayload).Path)
			}
			if targets.err != nil || !reflect.DeepEqual(got, entry.want) {
				t.Fatalf("delivery classification changed: %v, %v", got, targets.err)
			}
		}
	})
	t.Run("canonical pages publications and fork scope", func(t *testing.T) {
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		var branch protocol.Session
		var oldImage protocol.ConversationImage
		var newest uint64
		artifact := protocol.ArtifactSummary{ID: protocol.NewArtifactID(), Kind: protocol.ArtifactKindDocument, Session: session.ID, Turn: turn, Package: protocol.PluginReference{Name: "document-fixture", Digest: strings.Repeat("a", 64)}, Title: "HTML report", Revision: 1, Size: 16}
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			user := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "See [Input](input.txt)"))), now)
			user.Provenance.Author = "user"
			if _, _, err := AppendCanonical(ctx, db, session.ID, user); err != nil {
				return err
			}
			digest, err := PutBlob(ctx, db, []byte("canonical image bytes"))
			if err != nil {
				return err
			}
			content := protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{
				{Kind: "image", Data: &protocol.ToolContentBlockImagePayload{Index: 0}},
				{Kind: "file", Data: &protocol.ToolContentFile{Path: "output/report.pdf", Mime: "application/pdf"}},
				{Kind: "artifact", Data: &protocol.ToolContentBlockArtifactPayload{Artifact: artifact}},
				{Kind: "artifact", Data: &protocol.ToolContentBlockArtifactPayload{Artifact: artifact}},
			}}
			body, err := encode(map[string]any{"sailry_content": content})
			if err != nil {
				return err
			}
			body, err = encode(agentJSON(body))
			if err != nil {
				return err
			}
			message := canonical.NewMessage(canonical.Assistant,
				canonical.Block{Index: 0, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: digest, Mime: "image/png", Size: uint64(len("canonical image bytes"))}}},
				canonical.Block{Index: 1, Kind: "tool_response", Data: &canonical.ToolResponse{ID: "publish", Name: "fixture_publish", Status: canonical.ToolSuccess, Revision: fmt.Sprintf("%x", blake3.Sum256(body)), Result: canonical.Payload{Value: body}, Attachments: []canonical.Attachment{{Asset: canonical.AssetRef{Digest: digest, Mime: "image/png", Size: uint64(len("canonical image bytes"))}}}}},
			)
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			entry, _, err := AppendCanonical(ctx, db, session.ID, record)
			if err != nil {
				return err
			}
			oldImage = *entry.Parts[0].Data.(*protocol.ConversationImage)
			if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			branch, _, err = forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: turn, ExpectedRevision: session.Revision})
			if err != nil {
				return err
			}
			current, err := ReadSession(ctx, db, session.ID)
			if err != nil {
				return err
			}
			request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: current.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: "Later"}}})
			body, err = encode(request)
			if err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
				return err
			}
			capture, err := admitAgentTurn(ctx, db, node, node, request, current, protocol.ConversationRunKindTask, true)
			if err != nil {
				return err
			}
			if _, _, err = ClaimNext(ctx, db, now); err != nil {
				return err
			}
			for i := 0; i < 260; i++ {
				text := "No delivered resource"
				if i == 259 {
					text = "[New](new.txt)"
				}
				record := canonical.NewRecord(capture.ID, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, text))), now)
				record.Provenance.Author = "assistant"
				entry, _, err := AppendCanonical(ctx, db, session.ID, record)
				if err != nil {
					return err
				}
				newest = entry.Sequence
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
		err = storage.Node.Read(ctx, func(db *sql.Conn) error {
			query := protocol.ConversationAssetsQuery{Limit: 1}
			groups := []protocol.ConversationAssetsGroup{}
			pages := 0
			for {
				page, err := ListConversationAssets(ctx, db, session.ID, query)
				if err != nil {
					return err
				}
				groups = append(groups, page.Groups...)
				pages++
				if page.NextBefore == nil {
					break
				}
				if pages > 6 || query.Before != nil && *page.NextBefore >= *query.Before {
					return errors.New("asset pagination did not advance")
				}
				query.Before = page.NextBefore
			}
			if pages < 3 || len(groups) != 3 || groups[0].Sequence != newest || groups[1].Worktree != session.Worktree || groups[2].Kind != protocol.ConversationAssetsKindResource || len(groups[1].Items) != 4 {
				return fmt.Errorf("sparse inventory lost canonical groups: %d pages, %+v", pages, groups)
			}
			inherited, err := ListConversationAssets(ctx, db, branch.ID, protocol.ConversationAssetsQuery{Limit: 20})
			if err != nil {
				return err
			}
			if len(inherited.Groups) != 2 || inherited.Groups[0].Turn != turn || !reflect.DeepEqual(*inherited.Groups[0].Items[0].Data.(*protocol.ConversationImage), oldImage) {
				return fmt.Errorf("fork inventory lost canonical image provenance: %+v", inherited)
			}
			for _, query := range []protocol.ConversationAssetsQuery{{Limit: 0}, {Limit: 51}, {Limit: 1, Before: ptr(uint64(0))}} {
				if _, err := ListConversationAssets(ctx, db, session.ID, query); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
					return fmt.Errorf("invalid asset query was accepted: %+v, %v", query, err)
				}
			}
			if _, err = ListConversationAssets(ctx, db, branch.ID, protocol.ConversationAssetsQuery{Limit: 1, Before: &newest}); !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
				return fmt.Errorf("fork accepted an invisible cursor: %v", err)
			}
			kind := protocol.ConversationAssetsKindResource
			filtered, err := ListConversationAssets(ctx, db, branch.ID, protocol.ConversationAssetsQuery{Limit: 20, Kind: &kind})
			if err != nil || len(filtered.Groups) != 1 || filtered.Groups[0].Kind != kind {
				return fmt.Errorf("resource filter changed: %+v, %v", filtered, err)
			}
			plain := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_response", Data: &canonical.ToolResponse{ID: "write", Result: canonical.Payload{Value: json.RawMessage(`{"kind":"file_written","data":{"path":"source.go","revision":"captured","size":1}}`)}}})), now)
			items, err := conversationAssetItems(ctx, db, plain, session.Worktree, ptr("/project"))
			if err != nil || len(items) != 0 {
				return fmt.Errorf("ordinary write receipt became a delivery: %+v, %v", items, err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}
