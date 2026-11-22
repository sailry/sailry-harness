// Package output projects tool results only in outgoing model records. Canonical
// payloads and revisions remain immutable and readable through read_tool_output.
package output

import (
	"bytes"
	"cmp"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"lukechampine.com/blake3"
)

const MaxLines = 2000
const MaxBytes = 50 * 1024

type Cache struct {
	entries []entry
	reads   fileReads
}
type entry struct {
	identity identity
	format   canonical.ToolFormat
	value    json.RawMessage
	slots    slotProjection
}
type identity struct {
	message, call, originalCall, revision, name string
	visible, hasSlots                           bool
	digest                                      [32]byte
	format                                      canonical.ToolFormat
	status                                      canonical.ToolStatus
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
func object(value any) map[string]any                     { result, _ := value.(map[string]any); return result }
func array(value any) []any                               { result, _ := value.([]any); return result }
func text(value any) string                               { result, _ := value.(string); return result }
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

// Project borrows tool payloads until it selects the final output.
func cloneRecord(record canonical.Record, borrowToolValues bool) canonical.Record {
	msg := record.Message()
	if msg == nil {
		return clone(record)
	}
	shell := *msg
	shell.Content = nil
	record.Data = &shell
	copied := clone(record)
	out := copied.Message()
	if len(msg.Content) != 0 || borrowToolValues && msg.Content != nil {
		out.Content = make([]canonical.Block, len(msg.Content))
	}
	for i, block := range msg.Content {
		tool := response(&block)
		if tool == nil {
			out.Content[i] = clone(block)
			continue
		}
		fields := *tool
		fields.Result.Value = nil
		fields = clone(fields)
		if borrowToolValues {
			fields.Result.Value = tool.Result.Value
			fields.OriginalID = tool.OriginalID
		} else {
			raw, err := json.Marshal(tool.Result.Value)
			if err != nil {
				panic(err)
			}
			fields.Result.Value = raw
		}
		block.Data = &fields
		out.Content[i] = block
	}
	return copied
}
func (cache *Cache) Project(records []canonical.Record) []canonical.Record {
	if records == nil {
		*cache = Cache{}
		return nil
	}
	projected := make([]canonical.Record, len(records))
	for i := range records {
		projected[i] = cloneRecord(records[i], true)
	}
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
		response *canonical.ToolResponse
		identity identity
	}
	sources := []source{}
	for i := range projected {
		filterAudience(&projected[i], false)
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
				key := identity{
					message: record.ID, call: response.ID, originalCall: response.OriginalID, revision: response.Revision, name: response.Name,
					visible: isVisible, hasSlots: len(response.Result.Assets) != 0,
					digest: blake3.Sum256(response.Result.Value), format: response.Format, status: response.Status,
				}
				sources = append(sources, source{response: response, identity: key})
			}
		}
	}
	unchanged := len(sources) >= len(cache.entries)
	if unchanged {
		for i, entry := range cache.entries {
			if entry.identity.message == "" || entry.identity.revision == "" || entry.identity != sources[i].identity {
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
			cached := cache.entries[i]
			if cached.value != nil {
				source.response.Format = cached.format
				source.response.Result.Value = bytes.Clone(cached.value)
			} else {
				source.response.Result.Value = bytes.Clone(source.response.Result.Value)
			}
			cached.slots.apply(source.response)
			continue
		}
		original := source.response.Result.Value
		slots := slotProjection{audience: filterResultAudience(source.response)}
		projectResponse(source.response, source.identity.visible, &cache.reads, &slots)
		slots.apply(source.response)
		cached := entry{identity: source.identity, format: source.response.Format, slots: slots}
		if cached.format != source.identity.format || !bytes.Equal(original, source.response.Result.Value) {
			cached.value = bytes.Clone(source.response.Result.Value)
		} else {
			source.response.Result.Value = bytes.Clone(original)
		}
		cache.entries = append(cache.entries, cached)
	}
	return projected
}

func resultSource(response *canonical.ToolResponse) map[string]any {
	return map[string]any{"call": cmp.Or(response.OriginalID, response.ID), "revision": response.Revision}
}

func projectResponse(response *canonical.ToolResponse, visible bool, reads *fileReads, slots *slotProjection) {
	original := response.Result.Value
	name := commandToolName(response.Name)
	if projectToolFailure(response, name != "") {
		slots.clear, slots.clearAttachments = true, true
		return
	}
	source := resultSource(response)
	if name != "" && (projectCommand(response, name, source) || projectUnshapedCommand(response, source)) {
		return
	}
	if response.Format == canonical.MCPError {
		projectError(response, source)
		if !bytes.Equal(response.Result.Value, original) {
			slots.removedField = "/data"
		}
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
	preview(envelope, source, previous)
	slots.bounded = bound(envelope, source, tail, response.Format == canonical.JSON)
	if _, kept := envelope["structuredContent"]; !kept {
		slots.removedField = "/structuredContent"
	}
	if response.Format == canonical.JSON && reflect.DeepEqual(envelope, resultEnvelope(response)) {
		return
	}
	body := json.RawMessage(encoded(envelope))
	if response.Format == canonical.MCP && bytes.Equal(body, original) {
		return
	}
	slots.clear = response.Format != canonical.MCP
	response.Format = canonical.MCP
	response.Result.Value = body
}

// projectToolFailure replaces a diagnostic-only failure with its readable
// reason and the failure marker in model requests. A failure with other
// content keeps its bounded original body and attachments. The stored
// response remains available for inspection.
func projectToolFailure(response *canonical.ToolResponse, command bool) bool {
	if response.Status != canonical.ToolErrorStatus && response.Format != canonical.MCPError {
		return false
	}
	message := toolFailureText(response.Result.Value)
	if message == "" || !failureOnly(response) {
		return false
	}
	source := resultSource(response)
	if command {
		projectCommandText(response, source, message, true)
	} else if response.Format == canonical.MCPError {
		response.Result.Value = json.RawMessage(encoded(map[string]any{"message": message}))
		projectError(response, source)
	} else {
		envelope := map[string]any{
			"isError": true,
			"content": []any{map[string]any{"type": "text", "text": message}},
		}
		bound(envelope, source, false, false)
		response.Format = canonical.MCP
		response.Result.Value = json.RawMessage(encoded(envelope))
	}
	return true
}
func preview(envelope map[string]any, source map[string]any, reads *fileReads) {
	original := envelope["structuredContent"]
	fields := object(original)
	if fields == nil {
		return
	}
	if ref := object(fields["output"]); ref["call"] == source["call"] && ref["revision"] == source["revision"] {
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
