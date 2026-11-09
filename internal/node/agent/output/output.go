// Package output projects tool results only in outgoing model records. Canonical
// payloads and revisions remain immutable and readable through read_tool_output.
package output

import (
	"bytes"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

const MaxLines = 2000
const MaxBytes = 50 * 1024

type Cache struct {
	entries []entry
	reads   fileReads
}
type entry struct {
	identity  identity
	projected canonical.ToolResponse
}
type identity struct {
	message, call, revision string
	visible                 bool
	images                  int
	omitted                 string
}

func clone[T any](value T) T {
	raw, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	var result T
	if err := json.Unmarshal(raw, &result); err != nil {
		panic(err)
	}
	return result
}
func decode(raw json.RawMessage) any {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	if decoder.Decode(&value) != nil {
		return nil
	}
	return value
}
func encoded(value any) string {
	var buffer bytes.Buffer
	encoder := json.NewEncoder(&buffer)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(value); err != nil {
		panic(err)
	}
	return string(bytes.TrimSuffix(buffer.Bytes(), []byte{'\n'}))
}
func object(value any) map[string]any { result, _ := value.(map[string]any); return result }
func array(value any) []any           { result, _ := value.([]any); return result }
func text(value any) string           { result, _ := value.(string); return result }
func number(value any) uint64 {
	switch value := value.(type) {
	case json.Number:
		n, _ := value.Int64()
		if n > 0 {
			return uint64(n)
		}
	case float64:
		if value > 0 {
			return uint64(value)
		}
	case uint64:
		return value
	case int:
		if value > 0 {
			return uint64(value)
		}
	}
	return 0
}
func message(record *canonical.Record) *canonical.Message { return record.MutableMessage() }
func response(block *canonical.Block) *canonical.ToolResponse {
	if block.Kind != "tool_response" {
		return nil
	}
	switch value := block.Data.(type) {
	case *canonical.ToolResponse:
		return value
	case canonical.ToolResponse:
		block.Data = &value
		return &value
	}
	return nil
}
func resultEnvelope(response *canonical.ToolResponse) map[string]any {
	value := decode(response.Result.Value)
	if response.Format == canonical.MCP {
		return object(value)
	}
	if response.Format == canonical.MCPError {
		return nil
	}
	envelope := map[string]any{"content": []any{map[string]any{"type": "text", "text": encoded(value)}}, "structuredContent": value}
	if response.Status != canonical.ToolSuccess {
		envelope["isError"] = true
	}
	return envelope
}
func resultIdentity(record canonical.Record, response canonical.ToolResponse, visible bool) identity {
	identity := identity{message: record.ID, call: response.ID, revision: response.Revision, visible: visible}
	if envelope := resultEnvelope(&response); envelope != nil {
		for _, content := range array(envelope["content"]) {
			if object(content)["type"] == "image" {
				identity.images++
			}
		}
		if omitted := object(envelope["structuredContent"])["omitted_screenshots"]; omitted != nil {
			identity.omitted = encoded(omitted)
		}
	}
	return identity
}
func (cache *Cache) Project(records []canonical.Record) []canonical.Record {
	projected := clone(records)
	visible := map[string]bool{}
	for _, record := range projected {
		if record.Kind != "operation" {
			continue
		}
		operation, ok := record.Data.(*canonical.Operation)
		if !ok || operation.Kind != "visibility" {
			continue
		}
		if change, ok := operation.Data.(*canonical.Visibility); ok {
			visible[change.Message] = change.AgentVisible
		}
	}
	boundary := -1
	for i, record := range projected {
		if record.Kind != "compaction" {
			continue
		}
		if compact, ok := record.Data.(*canonical.Compaction); ok {
			for n := 0; n < i; n++ {
				if projected[n].ID == compact.Boundary {
					boundary = n
					break
				}
			}
		}
	}
	type source struct {
		record   int
		response *canonical.ToolResponse
		identity identity
	}
	sources := []source{}
	for i := range projected {
		audience(&projected[i])
		record := &projected[i]
		value := message(record)
		if value == nil {
			continue
		}
		isVisible := i > boundary && (record.Metadata.Runtime == nil || record.Metadata.Runtime.AgentVisible)
		if changed, exists := visible[record.ID]; exists {
			isVisible = i > boundary && changed
		}
		for j := range value.Content {
			if response := response(&value.Content[j]); response != nil {
				sources = append(sources, source{record: i, response: response, identity: resultIdentity(*record, *response, isVisible)})
			}
		}
	}
	unchanged := len(sources) >= len(cache.entries)
	if unchanged {
		for i, entry := range cache.entries {
			if entry.identity.message == "" || entry.identity.revision == "" || !reflect.DeepEqual(entry.identity, sources[i].identity) {
				unchanged = false
				break
			}
		}
	}
	if !unchanged {
		*cache = Cache{}
	}
	for i, source := range sources {
		if i < len(cache.entries) {
			*source.response = clone(cache.entries[i].projected)
			continue
		}
		projectResponse(source.response, source.identity.visible, &cache.reads)
		cache.entries = append(cache.entries, entry{identity: source.identity, projected: clone(*source.response)})
	}
	return projected
}
func projectResponse(response *canonical.ToolResponse, visible bool, reads *fileReads) {
	original := append(json.RawMessage(nil), response.Result.Value...)
	source := map[string]any{"call": response.ID, "revision": response.Revision}
	if response.Format == canonical.MCPError {
		projectError(response, source)
		return
	}
	envelope := resultEnvelope(response)
	if envelope == nil {
		return
	}
	value := envelope["structuredContent"]
	tail := false
	if kind := object(value)["kind"]; kind == "command_result" || kind == "command_output" {
		tail = true
	}
	var previous *fileReads
	if visible {
		previous = reads
	}
	preview(envelope, source, previous, response.ID)
	bound(envelope, source, tail, response.Format == canonical.JSON)
	if response.Format == canonical.JSON && reflect.DeepEqual(envelope, resultEnvelope(response)) {
		return
	}
	body := json.RawMessage(encoded(envelope))
	if response.Format == canonical.MCP && bytes.Equal(body, original) {
		return
	}
	response.Format = canonical.MCP
	response.Result = canonical.Payload{Value: body}
}
func preview(envelope map[string]any, source map[string]any, reads *fileReads, call string) {
	original := envelope["structuredContent"]
	fields := object(original)
	if fields == nil {
		return
	}
	if object(fields["output"])["call"] == call {
		return
	}
	var projected string
	ok := false
	if projected, ok = pagePreview(original); !ok {
		switch fields["kind"] {
		case "file_lines":
			projected, ok = filePreview(fields, source, reads)
		case "file_written":
			projected, ok = editPreview(fields)
		case "file_changes":
			projected, ok = changesPreview(fields)
		case "search_results":
			projected, ok = searchPreview(fields, source)
		}
	}
	if !ok {
		return
	}
	mirrored := false
	for _, content := range array(envelope["content"]) {
		part := object(content)
		if part["type"] == "text" && (text(part["text"]) == encoded(original) || reflect.DeepEqual(decode(json.RawMessage(text(part["text"]))), original)) {
			part["text"] = projected
			mirrored = true
		}
	}
	if !mirrored {
		envelope["content"] = append(array(envelope["content"]), map[string]any{"type": "text", "text": projected})
	}
	delete(envelope, "structuredContent")
}
