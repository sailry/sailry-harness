// Package tokens retains the source's offline o200k token accounting.
// Adapted from Goose ee8500d5 (Apache-2.0); see ../model/LICENSE.goose.
package tokens

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"
	"sync"

	"github.com/pkoukk/tiktoken-go"
	loader "github.com/pkoukk/tiktoken-go-loader"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

var tokenizer = sync.OnceValue(func() *tiktoken.Tiktoken {
	tiktoken.SetBpeLoader(loader.NewOfflineLoader())
	value, err := tiktoken.GetEncoding("o200k_base")
	if err != nil {
		panic(err)
	}
	return value
})

func Text(value string) uint64 { return uint64(len(tokenizer().Encode(value, []string{"all"}, nil))) }
func Chat(system string, records []canonical.Record) uint64 {
	var count uint64
	if system != "" {
		count = Text(system) + 4
	}
	for i := range records {
		if records[i].Metadata.Runtime != nil && !records[i].Metadata.Runtime.AgentVisible {
			continue
		}
		message := records[i].MutableMessage()
		if message == nil {
			continue
		}
		count += 4
		for _, block := range message.Content {
			switch value := block.Data.(type) {
			case *canonical.TextContent:
				count += Text(value.Text)
			case *canonical.ToolRequest:
				if value.Error != nil {
					continue
				}
				var arguments any
				decoder := json.NewDecoder(strings.NewReader(string(value.Arguments)))
				decoder.UseNumber()
				_ = decoder.Decode(&arguments)
				args := "None"
				if object, ok := arguments.(map[string]any); ok {
					args = "Some(" + debugObject(object) + ")"
				}
				count += Text(value.ID + ":" + value.Name + ":" + args)
			case *canonical.ToolResponse:
				if value.Format == canonical.MCPError {
					continue
				}
				var object map[string]any
				_ = json.Unmarshal(value.Result.Value, &object)
				var texts []string
				if value.Format == canonical.JSON {
					texts = append(texts, string(value.Result.Value))
				} else if content, ok := object["content"].([]any); ok {
					for _, part := range content {
						if part, ok := part.(map[string]any); ok && part["type"] == "text" {
							if text, ok := part["text"].(string); ok {
								texts = append(texts, text)
							}
						}
					}
				}
				if len(texts) > 0 {
					count += Text(strings.Join(texts, "\n"))
				}
			}
		}
	}
	return count + 3
}
func debugObject(object map[string]any) string {
	keys := make([]string, 0, len(object))
	for key := range object {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		raw, _ := json.Marshal(key)
		parts = append(parts, string(raw)+": "+debug(object[key]))
	}
	return "{" + strings.Join(parts, ", ") + "}"
}
func debug(value any) string {
	switch value := value.(type) {
	case nil:
		return "Null"
	case string:
		raw, _ := json.Marshal(value)
		return "String(" + string(raw) + ")"
	case bool:
		return fmt.Sprintf("Bool(%t)", value)
	case json.Number:
		return "Number(" + value.String() + ")"
	case map[string]any:
		return "Object " + debugObject(value)
	case []any:
		parts := make([]string, len(value))
		for i, v := range value {
			parts[i] = debug(v)
		}
		return "Array [" + strings.Join(parts, ", ") + "]"
	}
	panic("unsupported decoded JSON value")
}
