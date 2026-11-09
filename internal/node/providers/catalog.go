package providers

import (
	"context"
	"encoding/json"
	"net/http"
	"sort"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const catalogURL = "https://models.dev/api.json"

type CatalogEntry struct {
	Provider string
	ID       string
	Body     json.RawMessage
}
type CatalogDownload struct {
	Entries   []CatalogEntry
	Bytes     uint64
	Providers uint32
	Fault     *protocol.Fault
}

func downloadCatalog(ctx context.Context) (*CatalogDownload, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, catalogURL, nil)
	if err != nil {
		return nil, unavailable("model catalog HTTP client failed")
	}
	response, err := httpClient(30 * time.Second).Do(request)
	if err != nil {
		return nil, unavailable("model catalog download failed")
	}
	raw, err := readJSON(response, 32*1024*1024)
	if err != nil {
		return nil, err
	}
	return ParseCatalog(raw)
}
func ParseCatalog(raw []byte) (*CatalogDownload, error) {
	if len(raw) > 32*1024*1024 {
		return nil, unavailable("model catalog exceeds its size limit")
	}
	var document map[string]struct {
		Models map[string]json.RawMessage `json:"models"`
	}
	if err := json.Unmarshal(raw, &document); err != nil || len(document) == 0 || len(document) > 1000 {
		return nil, unavailable("model catalog providers are invalid")
	}
	download := &CatalogDownload{Bytes: uint64(len(raw)), Providers: uint32(len(document))}
	seen := map[string]bool{}
	for provider, data := range document {
		if !identifier(provider, 128) || data.Models == nil {
			return nil, unavailable("model catalog models are invalid")
		}
		for _, body := range data.Models {
			model, err := CatalogModel(body)
			if err != nil {
				return nil, err
			}
			key := provider + "\x00" + model.ID
			if seen[key] {
				return nil, unavailable("model catalog has duplicate model identifiers")
			}
			seen[key] = true
			if len(body) > 128*1024 || len(download.Entries) >= 100000 {
				return nil, unavailable("model catalog entries exceed their limits")
			}
			download.Entries = append(download.Entries, CatalogEntry{Provider: provider, ID: model.ID, Body: append(json.RawMessage(nil), body...)})
		}
	}
	sort.Slice(download.Entries, func(i, j int) bool {
		a, b := download.Entries[i], download.Entries[j]
		if a.Provider == b.Provider {
			return a.ID < b.ID
		}
		return a.Provider < b.Provider
	})
	return download, nil
}
func CatalogModel(raw json.RawMessage) (protocol.ConversationCatalogModel, error) {
	model := protocol.DefaultConversationCatalogModel()
	value := object(raw)
	if json.Unmarshal(value["id"], &model.ID) != nil || !identifier(model.ID, 256) {
		return model, unavailable("catalog model identifier is invalid")
	}
	model.Name = model.ID
	if field := value["name"]; len(field) > 0 && string(field) != "null" {
		if json.Unmarshal(field, &model.Name) != nil {
			return model, unavailable("catalog model name is invalid")
		}
	}
	model.Name = strings.TrimSpace(model.Name)
	if !identifier(model.Name, 512) {
		return model, unavailable("catalog model name is invalid")
	}
	for _, field := range []string{"limit", "modalities"} {
		if raw := value[field]; len(raw) > 0 && string(raw) != "null" && object(raw) == nil {
			return model, unavailable("catalog model metadata is invalid")
		}
	}
	limits := object(value["limit"])
	for _, item := range []struct {
		key    string
		target **uint32
	}{{"context", &model.Context}, {"output", &model.Output}} {
		if raw := limits[item.key]; len(raw) > 0 && string(raw) != "null" {
			var count uint32
			if json.Unmarshal(raw, &count) != nil {
				return model, unavailable("catalog token limit is invalid")
			}
			if count > 0 {
				*item.target = &count
			}
		}
	}
	for _, item := range []struct {
		key    string
		target **bool
	}{{"tool_call", &model.Tools}, {"reasoning", &model.Reasoning}} {
		if raw := value[item.key]; len(raw) > 0 && string(raw) != "null" {
			var flag bool
			if json.Unmarshal(raw, &flag) != nil {
				return model, unavailable("catalog capability is invalid")
			}
			*item.target = &flag
		}
	}
	modalities := object(value["modalities"])
	for _, item := range []struct {
		key    string
		target *[]string
	}{{"input", &model.Inputs}, {"output", &model.Outputs}} {
		if raw := modalities[item.key]; len(raw) > 0 && string(raw) != "null" {
			if json.Unmarshal(raw, item.target) != nil || len(*item.target) > 16 {
				return model, unavailable("catalog modalities are invalid")
			}
			for _, modality := range *item.target {
				if !identifier(modality, 32) {
					return model, unavailable("catalog modality is invalid")
				}
			}
		}
	}
	if raw := value["reasoning_options"]; len(raw) > 0 && string(raw) != "null" {
		if json.Unmarshal(raw, &model.Options) != nil || len(model.Options) > 16 {
			return model, unavailable("catalog reasoning options are invalid")
		}
		for _, option := range model.Options {
			switch option.Kind {
			case "effort":
				var values protocol.ConversationCatalogReasoningEffortPayload
				if err := unionData(option.Data, &values); err != nil || len(values.Values) > 32 {
					return model, unavailable("catalog reasoning values exceed their limit")
				}
				for _, value := range values.Values {
					if value != nil && !identifier(*value, 64) {
						return model, unavailable("catalog reasoning value is invalid")
					}
				}
			case "budget_tokens":
				var budget protocol.ConversationCatalogReasoningBudgetTokensPayload
				if err := unionData(option.Data, &budget); err != nil {
					return model, err
				}
				if budget.Min != nil && *budget.Min < -1 || budget.Min != nil && budget.Max != nil && *budget.Min >= 0 && uint32(*budget.Min) > *budget.Max {
					return model, unavailable("catalog reasoning budget is invalid")
				}
			}
		}
	}
	return model, nil
}
func identifier(value string, limit int) bool {
	if strings.TrimSpace(value) == "" || len(value) > limit {
		return false
	}
	for _, r := range value {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}
func unionData(source, target any) error {
	raw, err := json.Marshal(source)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, target)
}
