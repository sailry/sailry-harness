package model

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"
	"unicode/utf8"

	"charm.land/fantasy"
	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// MaxToolMediaBytes bounds one media item a tool result sends to a model.
	// It is the file-image bound and the per-image limit of the strictest API.
	MaxToolMediaBytes = 5 << 20

	toolImageOmitted = "(tool image omitted: model does not support images)"
	toolImageLarge   = "(tool image omitted: larger than 5 MiB)"
	toolImageNext    = "This tool result included an image that is uploaded in the next message."
)

var toolImageTypes = []string{"image/png", "image/jpeg", "image/gif", "image/webp"}

// ReadsImages reports whether the bound model takes image input: its listed
// capability, or the provider's default for a model the provider does not list,
// bounded by the adapter's declared input capabilities.
func ReadsImages(binding Binding) bool {
	if PluginBound(binding.Provider) && !(**binding.Provider.Binding).Adapter.Capabilities.Vision {
		return false
	}
	if model := metadata(binding); model != nil {
		return model.Vision
	}
	_, reads := MediaChannelFor(protocol.MediaKindVision, binding.Provider, protocol.ConversationModel{Vision: true})
	return reads
}

// toolMediaNote decides how one media item of a tool result reaches the bound
// model: as a media part, or as the note that replaces it.
func toolMediaNote(binding Binding, mime string, size uint64) (send bool, note string) {
	binary := fmt.Sprintf("[Binary content (%s) - %d bytes]", mime, size)
	switch {
	case slices.Contains(toolImageTypes, mime):
		if !ReadsImages(binding) {
			return false, toolImageOmitted
		}
		if size > MaxToolMediaBytes {
			return false, toolImageLarge
		}
		return true, ""
	case mime == "application/pdf" && binding.Provider.Api == protocol.ConversationModelApiAnthropic,
		mime != "" && binding.Provider.Api == protocol.ConversationModelApiGemini:
		return size <= MaxToolMediaBytes, binary
	}
	return false, binary
}

// SendsToolMedia reports whether a request to the bound model carries a tool
// media item of this type and size.
func SendsToolMedia(binding Binding, mime string, size uint64) bool {
	send, _ := toolMediaNote(binding, mime, size)
	return send
}

// toolContent is what one tool result says to the model: its text and the
// media parts that accompany it.
type toolContent struct {
	texts []string
	media []fantasy.FilePart
}

func (content *toolContent) note(text string) {
	if len(content.texts) == 0 || content.texts[len(content.texts)-1] != text {
		content.texts = append(content.texts, text)
	}
}

func (content *toolContent) add(binding Binding, mime string, data []byte) {
	if send, note := toolMediaNote(binding, mime, uint64(len(data))); send {
		content.media = append(content.media, fantasy.FilePart{MediaType: mime, Data: data})
	} else {
		content.note(note)
	}
}

// responseContent projects a tool result for the bound model. Image content of
// an MCP result and the attachments of a JSON result become media parts.
func responseContent(binding Binding, response canonical.ToolResponse, assets map[string][]byte) (string, []fantasy.FilePart, error) {
	raw, err := payloadText(response.Result, assets)
	if err != nil {
		return "", nil, err
	}
	var content toolContent
	var value map[string]json.RawMessage
	switch response.Format {
	case canonical.JSON:
		content.texts = []string{raw}
	case canonical.MCPError:
		if err := json.Unmarshal([]byte(raw), &value); err != nil {
			return "", nil, err
		}
		var message string
		json.Unmarshal(value["message"], &message)
		var code any
		json.Unmarshal(value["code"], &code)
		failure := fmt.Sprintf("Mcp error: %v: %s", code, message)
		if data := value["data"]; len(data) > 0 && string(data) != "null" {
			failure += " data: " + string(data)
		}
		return "The tool call returned the following error:\n" + failure, nil, nil
	default:
		if err := json.Unmarshal([]byte(raw), &value); err != nil {
			return "", nil, err
		}
		var parts []map[string]json.RawMessage
		if err := json.Unmarshal(value["content"], &parts); err != nil {
			return "", nil, err
		}
		for _, part := range parts {
			var kind, encoded, mime string
			json.Unmarshal(part["type"], &kind)
			switch kind {
			case "text":
				var text string
				json.Unmarshal(part["text"], &text)
				content.texts = append(content.texts, text)
			case "image", "audio":
				json.Unmarshal(part["data"], &encoded)
				json.Unmarshal(part["mimeType"], &mime)
				if data, err := base64.StdEncoding.DecodeString(encoded); err == nil {
					content.add(binding, mime, data)
				}
			case "resource":
				var resource map[string]json.RawMessage
				json.Unmarshal(part["resource"], &resource)
				if resource["text"] != nil {
					var text string
					json.Unmarshal(resource["text"], &text)
					content.texts = append(content.texts, text)
				} else if resource["blob"] != nil {
					json.Unmarshal(resource["blob"], &encoded)
					json.Unmarshal(resource["mimeType"], &mime)
					data, err := base64.StdEncoding.DecodeString(encoded)
					switch {
					case err != nil:
						content.texts = append(content.texts, encoded)
					case SendsToolMedia(binding, mime, uint64(len(data))):
						content.add(binding, mime, data)
					case utf8.Valid(data):
						content.texts = append(content.texts, string(data))
					default:
						if mime == "" {
							mime = "application/octet-stream"
						}
						content.add(binding, mime, data)
					}
				}
			}
		}
	}
	for _, attachment := range response.Attachments {
		asset := attachment.Asset
		send, note := toolMediaNote(binding, asset.Mime, asset.Size)
		if !send {
			content.note(note)
			continue
		}
		data, err := assetBytes(asset, assets)
		if err != nil {
			return "", nil, err
		}
		content.media = append(content.media, fantasy.FilePart{MediaType: asset.Mime, Data: data})
	}
	if response.Format == canonical.MCP && len(content.texts) == 0 && len(content.media) == 0 {
		if structured := value["structuredContent"]; len(structured) > 0 {
			return string(structured), nil, nil
		}
	}
	return strings.Join(content.texts, " "), content.media, nil
}

// toolFiles returns the media parts a tool message carries beside its result.
func toolFiles(message fantasy.Message) []fantasy.FilePart {
	if message.Role != fantasy.MessageRoleTool {
		return nil
	}
	var files []fantasy.FilePart
	for _, part := range message.Content {
		if file, ok := fantasy.AsMessagePart[fantasy.FilePart](part); ok {
			files = append(files, file)
		}
	}
	return files
}

func dataURL(file fantasy.FilePart) string {
	return "data:" + file.MediaType + ";base64," + base64.StdEncoding.EncodeToString(file.Data)
}

// toolMediaParts projects a stored MCP result's images for the bound model and
// appends the lifted media of a JSON result. It never changes canonical history.
func toolMediaParts(binding Binding, raw json.RawMessage, response canonical.ToolResponse, assets map[string][]byte) (json.RawMessage, error) {
	readsImages := ReadsImages(binding)
	if len(response.Attachments) == 0 && (readsImages || response.Format != canonical.MCP) {
		return raw, nil
	}
	var envelope map[string]json.RawMessage
	if json.Unmarshal(raw, &envelope) != nil {
		return raw, nil
	}
	var content []json.RawMessage
	_ = json.Unmarshal(envelope["content"], &content)
	changed := false
	if !readsImages && response.Format == canonical.MCP {
		content = slices.DeleteFunc(content, func(raw json.RawMessage) bool {
			var part struct {
				Type     string `json:"type"`
				Resource struct {
					Mime string          `json:"mimeType"`
					Blob json.RawMessage `json:"blob"`
				} `json:"resource"`
			}
			if json.Unmarshal(raw, &part) != nil {
				return false
			}
			image := part.Type == "image" || part.Type == "resource" && strings.HasPrefix(part.Resource.Mime, "image/") && len(part.Resource.Blob) > 0
			changed = changed || image
			return image
		})
	}
	for _, attachment := range response.Attachments {
		asset := attachment.Asset
		if !SendsToolMedia(binding, asset.Mime, asset.Size) || !slices.Contains(toolImageTypes, asset.Mime) {
			continue
		}
		data, err := assetBytes(asset, assets)
		if err != nil {
			return nil, err
		}
		part, _ := json.Marshal(map[string]any{"type": "image", "data": base64.StdEncoding.EncodeToString(data), "mimeType": asset.Mime})
		content = append(content, part)
		changed = true
	}
	if !changed {
		return raw, nil
	}
	envelope["content"], _ = json.Marshal(content)
	return json.Marshal(envelope)
}
