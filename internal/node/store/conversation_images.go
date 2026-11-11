package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func agentImageAssets(response *canonical.ToolResponse) []canonical.AssetRef {
	assets := []canonical.AssetRef{}
	for _, a := range response.Attachments {
		assets = append(assets, a.Asset)
	}
	if response.Format != canonical.JSON {
		return assets
	}
	envelope := agentObject(agentJSON(response.Result.Value))
	prefix := "/inline_data"
	if _, ok := envelope["data"]; ok && (envelope["kind"] == "computer" || envelope["kind"] == "browser" || envelope["kind"] == "external_browser" || envelope["kind"] == "file_image") {
		envelope = agentObject(envelope["data"])
		prefix = "/data/inline_data"
	}
	items, _ := envelope["inline_data"].([]any)
	for i := range items {
		pointer := fmt.Sprintf("%s/%d/data", prefix, i)
		for _, slot := range response.Result.Assets {
			if slot.Pointer == pointer {
				assets = append(assets, slot.Asset)
				break
			}
		}
	}
	return assets
}
func supportedAgentImage(asset canonical.AssetRef) bool {
	return strings.HasPrefix(asset.Mime, "image/") && len(asset.Mime) <= 255 && asset.Size > 0 && asset.Size <= 64*1024*1024
}
func describeAgentImage(turn protocol.TurnID, worktree protocol.WorktreeID, entry string, part, index uint64, asset canonical.AssetRef) (protocol.ConversationImage, error) {
	value := protocol.DefaultConversationImage()
	body, err := encode([]any{turn, entry, part, index, asset.Mime, asset.Digest})
	if err != nil {
		return value, err
	}
	sum := make([]byte, 32)
	blake3.DeriveKey(sum, "Sailry canonical image v1", body)
	var id protocol.AttachmentID
	copy(id[:], sum[:16])
	id[6] = (id[6] & 0x0f) | 0x80
	id[8] = (id[8] & 0x3f) | 0x80
	extension := map[string]string{"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif", "image/bmp": "bmp", "image/tiff": "tiff", "image/svg+xml": "svg"}[asset.Mime]
	if extension == "" {
		extension = "img"
	}
	value.Entry, value.Part, value.Index = entry, part, index
	value.Attachment = protocol.Attachment{ID: id, Spec: protocol.AttachmentSpec{Worktree: worktree, Name: fmt.Sprintf("image-%d.%s", index+1, extension), MediaType: asset.Mime, Size: asset.Size, Revision: asset.Digest}}
	return value, nil
}
func projectAgentImages(ctx context.Context, db *sql.Conn, record canonical.Record, block canonical.Block, response *canonical.ToolResponse) ([]protocol.ConversationImage, error) {
	values := []protocol.ConversationImage{}
	var worktree *protocol.WorktreeID
	for i, asset := range agentImageAssets(response) {
		if supportedAgentImage(asset) {
			if worktree == nil {
				run, err := ReadAgentRun(ctx, db, record.Turn)
				if err != nil {
					return nil, err
				}
				worktree = &run.Worktree
			}
			value, e := describeAgentImage(record.Turn, *worktree, record.ID, block.Index, uint64(i), asset)
			if e != nil {
				return nil, e
			}
			values = append(values, value)
		}
	}
	return values, nil
}
func ReadCanonicalImage(ctx context.Context, db *sql.Conn, session protocol.SessionID, image protocol.ConversationImage) ([]byte, error) {
	record, err := readBody[canonical.Record](ctx, db, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2", session.String(), image.Entry)
	if err != nil {
		return nil, err
	}
	missing := func() error { return fault(protocol.ErrorCodeNotFound, "image is unavailable in this conversation") }
	if record == nil || record.Message() == nil {
		return nil, missing()
	}
	var asset *canonical.AssetRef
	for _, block := range record.Message().Content {
		if block.Index != image.Part {
			continue
		}
		switch v := block.Data.(type) {
		case *canonical.ToolResponse:
			assets := agentImageAssets(v)
			if image.Index < uint64(len(assets)) {
				asset = &assets[image.Index]
			}
		case *canonical.AssetContent:
			if block.Kind == "image" && image.Index == 0 {
				asset = &v.Asset
			}
		}
	}
	if asset == nil || !supportedAgentImage(*asset) {
		return nil, missing()
	}
	data, err := GetBlob(ctx, db, asset.Digest)
	if err != nil {
		return nil, err
	}
	sum := blake3.Sum256(data)
	if uint64(len(data)) != asset.Size || hex.EncodeToString(sum[:]) != asset.Digest {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "image data changed")
	}
	run, err := visibleAgentRun(ctx, db, session, record.Turn)
	if err != nil {
		return nil, err
	}
	current, err := describeAgentImage(record.Turn, run.Worktree, record.ID, image.Part, image.Index, *asset)
	if err != nil {
		return nil, err
	}
	if current != image {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "image metadata changed")
	}
	return data, nil
}
