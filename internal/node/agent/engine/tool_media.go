package engine

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// mediaOutputs are the Node outputs whose data object may declare inline_data.
var mediaOutputs = []string{"computer", "browser", "external_browser", "file_image"}

// hydrateToolResult restores one stored tool result for the turn's history.
// Media a JSON result declares in inline_data leaves the value and becomes
// the attachments of the in-memory result, so a request carries it as media
// and never as base64 text. The attachments a tool stored for display are
// not model input and an MCP result carries its media in its own content, so
// neither stays on the result here.
func hydrateToolResult(ctx context.Context, db *sql.Conn, result *canonical.ToolResponse) error {
	payload := result.Result
	result.Attachments = nil
	if result.Format == canonical.JSON {
		payload, result.Attachments = liftToolMedia(payload)
	}
	value, err := store.HydrateCanonicalPayload(ctx, db, payload)
	if err != nil {
		return err
	}
	result.Result = canonical.Payload{Value: value}
	return nil
}

// liftToolMedia removes inline_data from the result root, or from the data of
// a typed Node output, and returns its stored assets for the model. A list
// with an item that was not stored as an asset stays in the value unchanged.
func liftToolMedia(payload canonical.Payload) (canonical.Payload, []canonical.Attachment) {
	if len(payload.Assets) == 0 {
		return payload, nil
	}
	var value any
	decoder := json.NewDecoder(bytes.NewReader(payload.Value))
	decoder.UseNumber()
	if decoder.Decode(&value) != nil {
		return payload, nil
	}
	envelope, _ := value.(map[string]any)
	prefix := "/inline_data/"
	if kind, _ := envelope["kind"].(string); slices.Contains(mediaOutputs, kind) {
		if data, ok := envelope["data"].(map[string]any); ok {
			envelope, prefix = data, "/data/inline_data/"
		}
	}
	items, _ := envelope["inline_data"].([]any)
	if len(items) == 0 {
		return payload, nil
	}
	media := make([]canonical.Attachment, 0, len(items))
	for index := range items {
		pointer := fmt.Sprintf("%s%d/data", prefix, index)
		at := slices.IndexFunc(payload.Assets, func(slot canonical.BinarySlot) bool { return slot.Pointer == pointer })
		if at < 0 {
			return payload, nil
		}
		// Media annotated for another audience is not model input.
		item, _ := items[index].(map[string]any)
		annotations, _ := item["annotations"].(map[string]any)
		if roles, limited := annotations["audience"].([]any); limited && !slices.Contains(roles, any("assistant")) {
			continue
		}
		media = append(media, canonical.Attachment{Asset: payload.Assets[at].Asset})
	}
	delete(envelope, "inline_data")
	body, err := json.Marshal(value)
	if err != nil {
		return payload, nil
	}
	remaining := slices.DeleteFunc(slices.Clone(payload.Assets), func(slot canonical.BinarySlot) bool { return strings.HasPrefix(slot.Pointer, prefix) })
	return canonical.Payload{Value: body, Assets: remaining}, media
}

// toolMediaAssets loads the lifted tool media the request sends to this model.
func (r *Runtime) toolMediaAssets(ctx context.Context, binding model.Binding, records []canonical.Record, assets map[string][]byte) error {
	return r.db.Read(ctx, func(db *sql.Conn) error {
		for i := range records {
			message := records[i].MutableMessage()
			if message == nil {
				continue
			}
			for _, block := range message.Content {
				response, ok := block.Data.(*canonical.ToolResponse)
				if !ok {
					continue
				}
				for _, attachment := range response.Attachments {
					asset := attachment.Asset
					if _, exists := assets[asset.Digest]; exists || !model.SendsToolMedia(binding, asset.Mime, asset.Size) {
						continue
					}
					data, err := store.GetBlob(ctx, db, asset.Digest)
					if err != nil {
						return err
					}
					assets[asset.Digest] = data
				}
			}
		}
		return nil
	})
}
