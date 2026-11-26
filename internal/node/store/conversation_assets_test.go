package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
	"lukechampine.com/blake3"
)

func TestConversationAssetGeneratedImageAliases(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	firstPath, secondPath := "assets/generated/first.png", "assets/generated/second.png"
	firstBytes, secondBytes := []byte("first generated image"), []byte("second generated image")
	var preview protocol.ConversationImage
	var generatedEntry string
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		assets := []canonical.AssetRef{}
		for _, body := range [][]byte{firstBytes, secondBytes} {
			digest, err := PutBlob(ctx, db, body)
			if err != nil {
				return err
			}
			assets = append(assets, canonical.AssetRef{Digest: digest, Mime: "image/png", Size: uint64(len(body))})
		}
		input := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "See [Input]("+firstPath+")"))), now)
		input.Provenance.Author = "user"
		if _, _, err := AppendCanonical(ctx, db, session.ID, input); err != nil {
			return err
		}
		blocks := []canonical.Block{}
		for index, path := range []string{firstPath, secondPath} {
			response := conversationGeneratedImageResponse(path, assets[index])
			response.ID = fmt.Sprintf("generate-%d", index)
			blocks = append(blocks, canonical.Block{Index: uint64(index), Kind: "tool_response", Data: response})
		}
		// A screenshot and a standalone image keep their own targets even when
		// their bytes match one of the generated files.
		screenshot := conversationGeneratedImageResponse(firstPath, assets[0])
		screenshot.ID, screenshot.Name = "screenshot", "computer"
		blocks = append(blocks,
			canonical.Block{Index: 2, Kind: "tool_response", Data: screenshot},
			canonical.Block{Index: 3, Kind: "image", Data: &canonical.AssetContent{Asset: assets[0]}},
		)
		generated := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, blocks...)), now.Add(time.Millisecond))
		generated.Provenance.Author = "tool"
		entry, _, err := AppendCanonical(ctx, db, session.ID, generated)
		if err != nil {
			return err
		}
		generatedEntry = entry.ID
		preview = entry.Parts[0].Data.(*protocol.ConversationPartToolResultPayload).Images[0]
		publication := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0,
			"![First]("+firstPath+")\n\n![Second]("+secondPath+")"))), now.Add(2*time.Millisecond))
		publication.Provenance.Author = "assistant"
		_, _, err = AppendCanonical(ctx, db, session.ID, publication)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		page, err := ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20})
		if err != nil {
			return err
		}
		if len(page.Groups) != 3 || page.Groups[0].Kind != protocol.ConversationAssetsKindArtifact || page.Groups[1].Kind != protocol.ConversationAssetsKindArtifact || page.Groups[2].Kind != protocol.ConversationAssetsKindResource {
			return fmt.Errorf("generated output changed inventory ownership: %+v", page.Groups)
		}
		for group, paths := range [][]string{{firstPath, secondPath}, {firstPath, secondPath}, {firstPath}} {
			items := page.Groups[group].Items
			wantCount := len(paths)
			if group == 1 {
				wantCount += 2
			}
			if len(items) != wantCount || page.Groups[group].Worktree != session.Worktree {
				return fmt.Errorf("generated output lost scoped targets: %+v", page.Groups[group])
			}
			for index, path := range paths {
				file, ok := items[index].Data.(*protocol.ConversationAssetsTargetFilePayload)
				if items[index].Kind != "file" || !ok || file.Path != path {
					return fmt.Errorf("generation and publication did not share file identity: %+v", items[index])
				}
			}
		}
		for index, part := range []uint64{2, 3} {
			item := page.Groups[1].Items[index+2]
			image, ok := item.Data.(*protocol.ConversationImage)
			if item.Kind != "image" || !ok || image.Entry != generatedEntry || image.Part != part {
				return fmt.Errorf("independent image was merged into a generated file: %+v", item)
			}
		}
		body, err := ReadCanonicalImage(ctx, db, session.ID, preview)
		if err != nil || !bytes.Equal(body, firstBytes) {
			return fmt.Errorf("inventory changed canonical preview access: %q, %v", body, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestGeneratedImageAssetFileRequiresNativeReceipt(t *testing.T) {
	t.Parallel()
	asset := canonical.AssetRef{Digest: strings.Repeat("a", 64), Mime: "image/png", Size: 10}
	for _, test := range []struct {
		name   string
		change func(*canonical.ToolResponse)
		want   string
	}{
		{"native", func(*canonical.ToolResponse) {}, "assets/generated/image.png"},
		{"literal path", func(response *canonical.ToolResponse) {
			*response = *conversationGeneratedImageResponse("assets/generated/a%20b#1?.png", asset)
		}, "assets/generated/a%20b#1?.png"},
		{"other tool", func(response *canonical.ToolResponse) { response.Name = "fixture_publish" }, ""},
		{"failed generation", func(response *canonical.ToolResponse) { response.Status = canonical.ToolErrorStatus }, ""},
		{"MCP result", func(response *canonical.ToolResponse) { response.Format = canonical.MCP }, ""},
		{"different size", func(response *canonical.ToolResponse) { response.Attachments[0].Asset.Size++ }, ""},
		{"different MIME", func(response *canonical.ToolResponse) { response.Attachments[0].Asset.Mime = "image/jpeg" }, ""},
		{"multiple images", func(response *canonical.ToolResponse) {
			response.Attachments = append(response.Attachments, response.Attachments[0])
		}, ""},
		{"outside worktree", func(response *canonical.ToolResponse) {
			*response = *conversationGeneratedImageResponse("../image.png", asset)
		}, ""},
		{"encoded JSON string", func(response *canonical.ToolResponse) {
			response.Result.Value, _ = json.Marshal(string(response.Result.Value))
		}, ""},
	} {
		t.Run(test.name, func(t *testing.T) {
			response := conversationGeneratedImageResponse("assets/generated/image.png", asset)
			test.change(response)
			path, ok := generatedImageAssetFile(response)
			if path != test.want || ok != (test.want != "") {
				t.Fatalf("ambiguous generation preview alias: %q, %v", path, ok)
			}
		})
	}
}

func conversationGeneratedImageResponse(path string, asset canonical.AssetRef) *canonical.ToolResponse {
	body, _ := json.Marshal(map[string]any{"path": path, "media_type": asset.Mime, "size": asset.Size, "provider": "fixture-provider", "model": "fixture-model"})
	return &canonical.ToolResponse{
		ID: "generate", Name: "generate_image", Status: canonical.ToolSuccess, Revision: fmt.Sprintf("%x", blake3.Sum256(body)),
		Result: canonical.Payload{Value: body}, Attachments: []canonical.Attachment{{Asset: asset}},
	}
}

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
	t.Run("publications and navigation", func(t *testing.T) {
		for _, entry := range []struct {
			input bool
			text  string
			want  []string
		}{
			{false, "[Report](report.md)\n\n![Image](image.png)\n\n[Again](./report.md)\n\nSource: [Code](src/main.rs#L3) `![Code](code.png)` [Outside](../secret) [Web](https://example.test/a.png) plain.txt", []string{"report.md", "image.png"}},
			{false, "[Budget](output/budget.xlsx)\n\n[Page](site/index.html)\n\n[Chart](chart.png)\n\n[Source](main.go)\n\n[Config](config.json)", []string{"output/budget.xlsx", "site/index.html", "chart.png"}},
			{false, "[Line](src/main.rs:42)\n\n[Section](guide.md#intro)\n\n[One](a.json) [Two](b.json)\n\nSee [Report](report.pdf)\n\n> [Source](src/quoted.rs)\n\n> > [Nested](src/nested.rs)\n\n`[Code](code.py)`\n\n[Web](https://example.test) [Mixed](mixed.json)\n\n- [Report](report.pdf)\n- [Data](data.json)", []string{}},
			{false, "Review [Inline](export.py#L7).\n\n- [List](stock.py)\n\n> [Quoted](test_export.py)\n\n[Top level](export.py)", []string{}},
			{true, "See [Input](input.txt)\n\n> [Quoted](quoted.txt)", []string{"input.txt", "quoted.txt"}},
		} {
			targets := &conversationAssetTargets{}
			conversationAssetMarkdown(targets, entry.text, ptr("/project"), entry.input)
			got := []string{}
			for _, item := range targets.items {
				got = append(got, item.Data.(*protocol.ConversationAssetsTargetFilePayload).Path)
			}
			if targets.err != nil || !reflect.DeepEqual(got, entry.want) {
				t.Fatalf("publication classification changed: %v, %v", got, targets.err)
			}
		}
	})
	t.Run("canonical pages publications and fork scope", func(t *testing.T) {
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		var branch protocol.Session
		var oldImage protocol.ConversationImage
		var newest uint64
		attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{
			Worktree: session.Worktree, Name: "source.txt", MediaType: "text/plain", Size: 1, Revision: strings.Repeat("a", 64),
		}}
		artifact := protocol.ArtifactSummary{ID: protocol.NewArtifactID(), Kind: protocol.ArtifactKindDocument, Session: session.ID, Turn: turn, Package: protocol.PluginReference{Name: "document-fixture", Digest: strings.Repeat("a", 64)}, Title: "HTML report", Revision: 1, Size: 16}
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			if _, err := db.ExecContext(ctx, "UPDATE worktrees SET path='/fixture/source' WHERE id=?1", session.Worktree.String()); err != nil {
				return err
			}
			if err := InsertAttachment(ctx, db, node, attachment); err != nil {
				return err
			}
			if err := BindTurnAttachments(ctx, db, turn, []protocol.AttachmentID{attachment.ID}); err != nil {
				return err
			}
			user := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User,
				canonical.Text(0, "[Reference context](hidden-reference.pdf)"),
				canonical.Text(1, "[Capability context](hidden-capability.pdf)"),
				canonical.Text(2, "[Notice context](hidden-notice.pdf)"),
				canonical.Text(3, "See [Input](/fixture/source/input.txt)"),
				canonical.Block{Index: 4, Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: "sailry-attachment://" + attachment.ID.String()}}},
			)), now)
			user.Provenance.Author = "user"
			user.Metadata.Input = &canonical.Input{CapabilityPart: ptr(uint64(1)), NoticePart: ptr(uint64(2)), References: []protocol.ConversationReference{
				{Target: protocol.ConversationReferenceTarget{Kind: "file", Data: ptr("referenced.md")}, Label: "Referenced file"},
			}}
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
			branch.Worktree = protocol.NewWorktreeID()
			if _, err = db.ExecContext(ctx, "INSERT INTO worktrees(id,path,main) VALUES(?1,'/fixture/branch',0)", branch.Worktree.String()); err != nil {
				return err
			}
			if _, _, err = reviseSession(ctx, db, branch); err != nil {
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
				message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "No published resource"))
				if i == 0 {
					message = canonical.NewMessage(canonical.Assistant, canonical.Text(0,
						"Review [Inline](export.py#L7).\n\n- [List](stock.py)\n\n> [Quoted](test_export.py)\n\n[Top level](export.py)"))
				}
				if i == 259 {
					body, err := encode(agentJSON(json.RawMessage(`{"sailry_content":{"version":1,"blocks":[{"kind":"file","path":"new.txt","mime":"text/plain"}]}}`)))
					if err != nil {
						return err
					}
					message = canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_response", Data: &canonical.ToolResponse{
						ID: "publish-new", Name: "fixture_publish", Status: canonical.ToolSuccess,
						Revision: fmt.Sprintf("%x", blake3.Sum256(body)), Result: canonical.Payload{Value: body},
					}})
				}
				record := canonical.NewRecord(capture.ID, "message", &message, now)
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
			var reads ConversationAssetReads
			tracked := protocol.ConversationAssetsQuery{Limit: 20, Checkpoint: ptr("")}
			parent, err := reads.List(ctx, db, session.ID, tracked)
			if err != nil {
				return err
			}
			tracked.Checkpoint = &parent.Checkpoint
			fork, err := reads.List(ctx, db, branch.ID, tracked)
			if err != nil || fork.Delta || fork.Checkpoint == "" || !reflect.DeepEqual(fork.Groups, inherited.Groups) {
				return fmt.Errorf("fork reused another conversation's checkpoint: %+v, %v", fork, err)
			}
			tracked.Checkpoint = &fork.Checkpoint
			warm, err := reads.List(ctx, db, branch.ID, tracked)
			if err != nil || !warm.Delta || len(warm.Groups) != 0 {
				return fmt.Errorf("inherited inventory did not validate at its origin: %+v, %v", warm, err)
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
			resource := filtered.Groups[0]
			want := []protocol.ConversationAssetsTarget{
				{Kind: "file", Data: &protocol.ConversationAssetsTargetFilePayload{Path: "input.txt"}},
				{Kind: "attachment", Data: &attachment},
				{Kind: "file", Data: &protocol.ConversationAssetsTargetFilePayload{Path: "referenced.md"}},
			}
			if resource.Worktree != session.Worktree || !reflect.DeepEqual(resource.Items, want) {
				return fmt.Errorf("fork resource targets lost origin scope: %+v", resource)
			}
			plain := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_response", Data: &canonical.ToolResponse{ID: "write", Result: canonical.Payload{Value: json.RawMessage(`{"kind":"file_written","data":{"path":"source.go","revision":"captured","size":1}}`)}}})), now)
			items, _, err := conversationAssetItems(ctx, db, plain, session.Worktree, ptr("/project"), nil)
			if err != nil || len(items) != 0 {
				return fmt.Errorf("ordinary write receipt became a delivery: %+v, %v", items, err)
			}
			review := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0,
				"Review [Inline](export.py#L7).\n\n- [List](stock.py)\n\n> [Quoted](test_export.py)\n\n[Top level](export.py)"))), now)
			review.Provenance.Author = "assistant"
			items, _, err = conversationAssetItems(ctx, db, review, session.Worktree, ptr("/project"), nil)
			if err != nil || len(items) != 0 {
				return fmt.Errorf("review references became published outputs: %+v, %v", items, err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
}

func TestConversationAssetStoredToolFormats(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	const file = `{"kind":"file","path":"report.pdf","mime":"application/pdf"}`
	const content = `{"version":1,"blocks":[` + file + `]}`
	const wrapped = `{"sailry_content":` + content + `}`
	const imageContent = `{"version":1,"blocks":[{"kind":"image","index":0},` + file + `]}`
	tests := []struct {
		name   string
		format canonical.ToolFormat
		value  string
		slot   string
		hidden bool
		items  []string
	}{
		{name: "direct", value: wrapped, items: []string{"file"}},
		{name: "escaped block kind", value: strings.Replace(wrapped, `"file"`, `"f\u0069le"`, 1), items: []string{"file"}},
		{name: "JSON structured", value: `{"structuredContent":` + wrapped + `}`, items: []string{"file"}},
		{name: "JSON output", value: `{"output":` + wrapped + `}`, items: []string{"file"}},
		{name: "MCP structured", format: canonical.MCP, value: `{"structuredContent":` + wrapped + `}`, items: []string{"file"}},
		{name: "MCP structured output", format: canonical.MCP, value: `{"structuredContent":{"output":` + wrapped + `}}`, items: []string{"file"}},
		{name: "MCP error output", format: canonical.MCPError, value: `{"output":` + wrapped + `}`, items: []string{"file"}},
		{name: "JSON structured null falls back", value: `{"structuredContent":{"sailry_content":null},"output":` + wrapped + `}`, items: []string{"file"}},
		{name: "JSON direct null takes precedence", value: `{"sailry_content":null,"output":` + wrapped + `}`},
		{name: "MCP structured null takes precedence", format: canonical.MCP, value: `{"structuredContent":null,"output":` + wrapped + `}`},
		{name: "escaped key", value: `{"log":"` + strings.Repeat("x", 8<<10) + `","sailry\u005fcontent":` + content + `}`, items: []string{"file"}},
		{name: "missing text", value: `{"sailry_content":{"version":1,"blocks":[{"kind":"text","path":"/text"},` + file + `]}}`},
		{name: "captured text", value: `{"output":{"sailry_content":{"version":1,"blocks":[{"kind":"text","path":"/detail"},` + file + `]},"detail":null}}`, slot: "/output/detail", items: []string{"file"}},
		{name: "legacy root image", value: `{"inline_data":[{"data":null}],"sailry_content":` + imageContent + `}`, slot: "/inline_data/0/data", items: []string{"image", "file"}},
		{name: "legacy typed image", value: `{"kind":"computer","data":{"inline\u005fdata":[{"data":null}]}}`, slot: "/data/inline_data/0/data", items: []string{"image"}},
		{name: "captured context", hidden: true, value: `{"inline_data":[{"data":null}],"sailry_content":` + imageContent + `}`, slot: "/inline_data/0/data", items: []string{"file"}},
	}
	sequences := make([]uint64, len(tests))
	imageBytes := []byte("fixture media")
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		digest, err := PutBlob(ctx, db, imageBytes)
		if err != nil {
			return err
		}
		for i, test := range tests {
			payload := canonical.Payload{Value: json.RawMessage(test.value)}
			if test.slot != "" {
				payload.Assets = []canonical.BinarySlot{{Pointer: test.slot, Asset: canonical.AssetRef{Digest: digest, Mime: "image/png", Size: uint64(len(imageBytes))}, Encoding: canonical.Base64}}
			}
			hydrated, err := HydrateCanonicalPayload(ctx, db, payload)
			if err != nil {
				return err
			}
			revision, err := encode(agentJSON(hydrated))
			if err != nil {
				return err
			}
			response := &canonical.ToolResponse{ID: fmt.Sprint(i), Name: "fixture_publish", Status: canonical.ToolSuccess, Format: test.format, Revision: fmt.Sprintf("%x", blake3.Sum256(revision)), Result: payload}
			message := canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: response})
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "tool"
			if test.hidden {
				message.Content = append(message.Content, canonical.Text(1, "![Hidden](hidden.png)"))
				message.PartCount++
				record.Provenance.Author = "system"
				record.Metadata.Context = &canonical.Context{Captured: true}
			}
			if sequences[i], err = insertConversationAssetFixture(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		page, err := ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 50})
		if err != nil {
			return err
		}
		groupIndex := 0
		for i := len(tests) - 1; i >= 0; i-- {
			test := tests[i]
			if len(test.items) == 0 {
				continue
			}
			if groupIndex >= len(page.Groups) || page.Groups[groupIndex].Sequence != sequences[i] {
				return fmt.Errorf("%s: missing or out of order in inventory: %+v", test.name, page.Groups)
			}
			items := page.Groups[groupIndex].Items
			if len(items) != len(test.items) {
				return fmt.Errorf("%s: unexpected targets: %+v", test.name, items)
			}
			for j, item := range items {
				if item.Kind != test.items[j] {
					return fmt.Errorf("%s: target %d = %s, want %s", test.name, j, item.Kind, test.items[j])
				}
				switch value := item.Data.(type) {
				case *protocol.ConversationAssetsTargetFilePayload:
					if value.Path != "report.pdf" {
						return fmt.Errorf("%s: unexpected publication %q", test.name, value.Path)
					}
				case *protocol.ConversationImage:
					body, err := ReadCanonicalImage(ctx, db, session.ID, *value)
					if err != nil || !bytes.Equal(body, imageBytes) {
						return fmt.Errorf("%s: legacy image bytes = %q, error = %v", test.name, body, err)
					}
				}
			}
			groupIndex++
		}
		if page.NextBefore != nil || groupIndex != len(page.Groups) {
			return fmt.Errorf("unexpected trailing inventory: %+v", page)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConversationAssetCursorUsesStoredBytes(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	value := json.RawMessage(`{"kind":"command_result","data":{"stdout":{"text":"` + strings.Repeat("x", 4<<20) + `"}}}`)
	var newest, oldest uint64
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		old := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "[Old](old.pdf)"))), now)
		old.Provenance.Author = "assistant"
		var err error
		if oldest, err = insertConversationAssetFixture(ctx, db, session.ID, old); err != nil {
			return err
		}
		response := &canonical.ToolResponse{ID: "log", Name: "fixture_output", Status: canonical.ToolSuccess, Revision: fmt.Sprintf("%x", blake3.Sum256(value)), Result: canonical.Payload{Value: value}}
		message := canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: response}, canonical.Text(1, "[New](new.pdf)"))
		record := canonical.NewRecord(turn, "message", &message, now)
		record.Provenance.Author = "tool"
		newest, err = insertConversationAssetFixture(ctx, db, session.ID, record)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		page, err := ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20})
		if err != nil {
			return err
		}
		if len(page.Groups) != 1 || page.Groups[0].Sequence != newest || page.NextBefore == nil || *page.NextBefore != newest {
			return fmt.Errorf("stored bytes no longer cap inventory pages: %+v", page)
		}
		next, err := ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Before: page.NextBefore})
		if err != nil {
			return err
		}
		if len(next.Groups) != 1 || next.Groups[0].Sequence != oldest || next.NextBefore != nil || next.Revision != page.Revision {
			return fmt.Errorf("byte-capped cursor did not resume at the next record: %+v", next)
		}
		kind := protocol.ConversationAssetsKindResource
		filtered, err := ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Kind: &kind})
		if err != nil || len(filtered.Groups) != 0 || filtered.NextBefore == nil || *filtered.NextBefore != newest {
			return fmt.Errorf("kind filter changed the byte-capped cursor: %+v, %v", filtered, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	var prefix uint64
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Small append"))), now)
		record.Provenance.Author = "assistant"
		var err error
		prefix, err = insertConversationAssetFixture(ctx, db, session.ID, record)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var reads ConversationAssetReads
		page, err := reads.List(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Checkpoint: ptr("")})
		if err != nil || len(page.Groups) != 0 || page.NextBefore == nil || *page.NextBefore != prefix || page.Checkpoint == "" {
			return fmt.Errorf("oversized second row crossed the byte budget: %+v, %v", page, err)
		}
		next, err := reads.List(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Before: page.NextBefore, Checkpoint: &page.Checkpoint})
		if err != nil || len(next.Groups) != 1 || next.Groups[0].Sequence != newest || next.NextBefore == nil || *next.NextBefore != newest {
			return fmt.Errorf("empty first page lost the oversized publication: %+v, %v", next, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConversationAssetEncodedPageBudget(t *testing.T) {
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var reads ConversationAssetReads
	list := func(query protocol.ConversationAssetsQuery) protocol.ConversationAssetsPage {
		t.Helper()
		var page protocol.ConversationAssetsPage
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			page, err = reads.List(ctx, db, session.ID, query)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if query.Checkpoint != nil && page.Checkpoint == "" {
			t.Fatal("missing checkpoint")
		}
		return page
	}
	kind := protocol.ConversationAssetsKindResource
	query := protocol.ConversationAssetsQuery{Limit: 20, Kind: &kind, Checkpoint: ptr("")}
	seed := list(query)
	// Two names fit in 16 MiB before response headroom, but not in the page budget.
	const padding = (protocol.MAX_FRAME_BYTES - 32*1024) / (2 * len(`\u003c`))
	large := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{
		Worktree: session.Worktree, Name: strings.Repeat("<", padding) + ".txt", MediaType: "text/plain", Size: 1, Revision: strings.Repeat("a", 64),
	}}
	note := large
	note.ID, note.Spec.Name = protocol.NewAttachmentID(), "deferred.txt"
	file := func(index uint64, id protocol.AttachmentID) canonical.Block {
		return canonical.Block{Index: index, Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: "sailry-attachment://" + id.String(), Mime: &large.Spec.MediaType}}}
	}
	var sequences [3]uint64
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		// Populate legacy metadata directly: canonical history stores only the references.
		body, err := encode(large)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)", large.ID.String(), session.Worktree.String(), node[:], body); err != nil {
			return err
		}
		if err := InsertAttachment(ctx, db, node, note); err != nil {
			return err
		}
		if err := BindTurnAttachments(ctx, db, turn, []protocol.AttachmentID{large.ID, note.ID}); err != nil {
			return err
		}
		for i, message := range []canonical.Message{
			canonical.NewMessage(canonical.User, file(0, large.ID), file(1, note.ID)),
			canonical.NewMessage(canonical.Assistant, canonical.Text(0, "[Filtered](filtered.pdf)")),
			canonical.NewMessage(canonical.User, file(0, large.ID)),
		} {
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = string(message.Role)
			if sequences[i], err = insertConversationAssetFixture(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	query.Checkpoint = &seed.Checkpoint
	first := list(query)
	if first.Delta || len(first.Groups) != 1 || first.Groups[0].Sequence != sequences[2] || first.NextBefore == nil || *first.NextBefore != sequences[1] {
		t.Fatalf("an incomplete delta must return a full page ending at the filtered record: %+v", first)
	}
	reply := wire.Reply{Kind: wire.ReplyCompleted, Completed: &wire.Response{Output: &protocol.Output{Kind: "conversation_assets", Data: first}}}
	if err := wire.Write(io.Discard, reply, false); err != nil {
		t.Fatalf("byte-capped page does not fit a Link frame: %v", err)
	}
	updateNote := func(name string) {
		t.Helper()
		note.Spec.Name = name
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			body, err := encode(note)
			if err != nil {
				return err
			}
			_, err = db.ExecContext(ctx, "UPDATE attachments SET body=?2 WHERE id=?1", note.ID.String(), body)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	updateNote("changed while deferred.txt")
	query.Before, query.Checkpoint = first.NextBefore, &first.Checkpoint
	next := list(query)
	if next.Delta || next.Revision != first.Revision || next.NextBefore != nil || len(next.Groups) != 1 || next.Groups[0].Sequence != sequences[0] || len(next.Groups[0].Items) != 2 {
		t.Fatalf("continuation did not deliver the deferred group: %+v", next)
	}
	if got, ok := next.Groups[0].Items[1].Data.(*protocol.Attachment); !ok || got.Spec.Name != note.Spec.Name {
		t.Fatal("continuation did not read the updated deferred attachment")
	}
	query.Before, query.Checkpoint = nil, &next.Checkpoint
	if warm := list(query); !warm.Delta || warm.NextBefore != nil || len(warm.Groups) != 0 {
		t.Fatalf("completed traversal did not retain the head: %+v", warm)
	}

	updateNote(large.Spec.Name)
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		page, err := ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20, Before: first.NextBefore})
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != "conversation asset group exceeds the response frame budget" {
			return fmt.Errorf("oversized group did not return an explicit frame fault: %v", err)
		}
		if page.NextBefore != nil || len(page.Groups) != 0 {
			return errors.New("oversized group consumed a cursor or emitted groups")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	artifact := protocol.ConversationAssetsKindArtifact
	filtered := list(protocol.ConversationAssetsQuery{Limit: 20, Kind: &artifact})
	if len(filtered.Groups) != 1 || filtered.Groups[0].Sequence != sequences[1] || filtered.NextBefore != nil {
		t.Fatalf("kind filter did not bypass the oversized group: %+v", filtered)
	}
}
