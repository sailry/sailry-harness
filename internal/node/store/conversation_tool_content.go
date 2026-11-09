package store

import (
	"encoding/json"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func agentContentPayload(result any) map[string]any {
	object := agentObject(result)
	if _, ok := object["sailry_content"]; ok {
		return object
	}
	if nested := agentObject(object["structuredContent"]); nested["sailry_content"] != nil {
		return nested
	}
	return agentObject(object["output"])
}
func (raw *rawAgentTool) content() *protocol.ToolContent {
	if raw.Presentation != protocol.ToolPresentationContent {
		return nil
	}
	payload := agentContentPayload(raw.value())
	encoded, err := json.Marshal(payload["sailry_content"])
	if err != nil || len(encoded) > 256*1024 {
		return nil
	}
	var value protocol.ToolContent
	if json.Unmarshal(encoded, &value) != nil || value.Version != 1 || len(value.Blocks) == 0 || len(value.Blocks) > 16 {
		return nil
	}
	images := map[uint64]bool{}
	size := 0
	for _, block := range value.Blocks {
		if raw.Failure != nil && block.Kind != "text" && block.Kind != "notice" && block.Kind != "card" {
			return nil
		}
		switch block.Kind {
		case "text":
			text := block.Data.(*protocol.ToolContentText)
			if !validAgentPointer(text.Path) || len(text.Notices) > 8 {
				return nil
			}
			body := agentString(agentPointer(payload, text.Path))
			if body == nil {
				return nil
			}
			size += len(*body)
			if size > 256*1024 {
				return nil
			}
			for _, n := range text.Notices {
				if !validAgentNavigation(n) {
					return nil
				}
			}
		case "notice":
			if !validAgentNavigation(block.Data.(*protocol.ToolContentBlockNoticePayload).Message) {
				return nil
			}
		case "table":
			t := block.Data.(*protocol.ToolContentTable)
			if len(t.Columns) == 0 || len(t.Columns) > 64 || len(t.Rows) > 1000 {
				return nil
			}
			for _, column := range t.Columns {
				if len(column) > 1024 {
					return nil
				}
			}
			for _, row := range t.Rows {
				if len(row) != len(t.Columns) {
					return nil
				}
				for _, cell := range row {
					if len(cell) > 16*1024 {
						return nil
					}
				}
			}
		case "image":
			index := block.Data.(*protocol.ToolContentBlockImagePayload).Index
			if images[index] {
				return nil
			}
			images[index] = true
			exists := false
			for _, image := range raw.Images {
				exists = exists || image.Index == index
			}
			if !exists {
				return nil
			}
		case "diff":
			diff := block.Data.(*protocol.ToolContentDiff)
			if diff.Path == "" || len(diff.Path) > 4096 || strings.IndexFunc(diff.Path, unicode.IsControl) >= 0 || strings.Count(strings.TrimSuffix(diff.Text, "\n"), "\n")+1 > 4096 {
				return nil
			}
		case "file":
			file := block.Data.(*protocol.ToolContentFile)
			if !validAgentContentFile(*file) {
				return nil
			}
		case "artifact", "card":
			// These are Node-stamped summaries. Their package owners validate resources
			// before canonical capture; the strict protocol codec checks stored shapes.
		default:
			return nil
		}
	}
	return &value
}
func validAgentNavigation(value protocol.PluginDesktopNavigation) bool {
	if strings.TrimSpace(value.Label) == "" || len(value.Label) > 128 || strings.IndexFunc(value.Label, unicode.IsControl) >= 0 || len(value.Locales) > 32 {
		return false
	}
	for locale, text := range value.Locales {
		if strings.TrimSpace(locale) == "" || len(locale) > 32 || strings.IndexFunc(locale, unicode.IsControl) >= 0 || strings.TrimSpace(text) == "" || len(text) > 128 || strings.IndexFunc(text, unicode.IsControl) >= 0 {
			return false
		}
	}
	return true
}
func validAgentPointer(path string) bool {
	if !strings.HasPrefix(path, "/") || len(path) > 1024 || strings.IndexFunc(path, unicode.IsControl) >= 0 {
		return false
	}
	for i := 0; i < len(path); i++ {
		if path[i] == '~' {
			i++
			if i >= len(path) || path[i] != '0' && path[i] != '1' {
				return false
			}
		}
	}
	return true
}
func validAgentContentFile(file protocol.ToolContentFile) bool {
	if file.Path == "" || len(file.Path) > 4096 || strings.ContainsAny(file.Path, "\\:") || strings.IndexFunc(file.Path, unicode.IsControl) >= 0 {
		return false
	}
	for _, part := range strings.Split(file.Path, "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	if file.Name != nil && (strings.TrimSpace(*file.Name) == "" || len(*file.Name) > 512 || strings.IndexFunc(*file.Name, unicode.IsControl) >= 0) {
		return false
	}
	parts := strings.SplitN(file.Mime, "/", 2)
	if len(parts) != 2 || parts[0] == "" || parts[1] == "" || len(file.Mime) > 128 {
		return false
	}
	for _, c := range file.Mime {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || strings.ContainsRune("/.-+", c)) {
			return false
		}
	}
	if file.Revision != nil {
		if *file.Revision == "" || len(*file.Revision) > 128 || !agentHex(*file.Revision) {
			return false
		}
	}
	return true
}
func agentContentDetails(value protocol.ToolContent, result any) bool {
	payload := agentContentPayload(result)
	for _, block := range value.Blocks {
		switch block.Kind {
		case "file", "artifact", "card":
			continue
		case "text":
			text := block.Data.(*protocol.ToolContentText)
			if len(text.Notices) > 0 {
				return true
			}
			if value := agentString(agentPointer(payload, text.Path)); value != nil && *value != "" {
				return true
			}
		default:
			return true
		}
	}
	return false
}
func agentHex(value string) bool {
	for _, c := range value {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') {
			return false
		}
	}
	return true
}
