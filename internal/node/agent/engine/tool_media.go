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

// modelToolResult detaches the result's presentation metadata. Declared JSON
// media becomes request-local attachment references; display-only attachments
// never reach the model and MCP carries media in its own content.
func modelToolResult(stored *canonical.ToolResponse) canonical.ToolResponse {
	result := *stored
	result.Attachments = nil
	if result.Format == canonical.JSON {
		result.Result, result.Attachments = liftToolMedia(result.Result)
	}
	return result
}

func (r *Runtime) readToolResult(ctx context.Context, stored *canonical.ToolResponse) (json.RawMessage, error) {
	if len(stored.Result.Assets) == 0 {
		return stored.Result.Value, nil
	}
	result := modelToolResult(stored)
	if len(result.Result.Assets) == 0 {
		return result.Result.Value, nil
	}
	var value json.RawMessage
	err := r.db.Read(ctx, func(db *sql.Conn) error {
		var err error
		value, err = store.HydrateCanonicalPayload(ctx, db, result.Result)
		return err
	})
	return value, err
}

// hydrateModelToolResults receives the detached, visible, answered history
// for one model request. Only affected messages and tool results are copied.
func (r *Runtime) hydrateModelToolResults(ctx context.Context, records []canonical.Record) error {
	var pending []*canonical.ToolResponse
	for i := range records {
		message := records[i].Message()
		if message == nil {
			continue
		}
		var content []canonical.Block
		for j, block := range message.Content {
			stored, ok := block.Data.(*canonical.ToolResponse)
			if !ok || len(stored.Attachments) == 0 && len(stored.Result.Assets) == 0 {
				continue
			}
			if content == nil {
				content = slices.Clone(message.Content)
			}
			result := modelToolResult(stored)
			content[j].Data = &result
			if len(result.Result.Assets) > 0 {
				pending = append(pending, &result)
			}
		}
		if content != nil {
			projected := *message
			projected.Content = content
			records[i].Data = &projected
		}
	}
	if len(pending) == 0 {
		return nil
	}
	return r.db.Read(ctx, func(db *sql.Conn) error {
		for _, result := range pending {
			value, err := store.HydrateCanonicalPayload(ctx, db, result.Result)
			if err != nil {
				return err
			}
			result.Result = canonical.Payload{Value: value}
		}
		return nil
	})
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
