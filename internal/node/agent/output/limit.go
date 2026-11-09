package output

import (
	"encoding/json"
	"fmt"
	"lukechampine.com/blake3"
	"reflect"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

func lineCount(value string) int {
	count := strings.Count(value, "\n")
	if value != "" && !strings.HasSuffix(value, "\n") {
		count++
	}
	return count
}
func head(value string, bytes, lines int) int {
	if lines == 0 || bytes <= 0 {
		return 0
	}
	end := len(value)
	if end > bytes {
		end = bytes
	}
	for end > 0 && end < len(value) && !utf8.RuneStart(value[end]) {
		end--
	}
	cursor := 0
	for n := 0; n < lines; n++ {
		at := strings.IndexByte(value[cursor:end], '\n')
		if at < 0 {
			break
		}
		cursor += at + 1
		if n == lines-1 {
			return cursor
		}
	}
	if end < len(value) {
		if at := strings.LastIndexByte(value[:end], '\n'); at >= 0 {
			return at + 1
		}
	}
	return end
}
func tailStart(value string, bytes, lines int) int {
	if lines == 0 || bytes <= 0 {
		return len(value)
	}
	start := len(value) - bytes
	if start < 0 {
		start = 0
	}
	for start < len(value) && !utf8.RuneStart(value[start]) {
		start++
	}
	before := strings.TrimSuffix(value, "\n")
	at := len(before)
	for n := 0; n < lines; n++ {
		index := strings.LastIndexByte(before[:at], '\n')
		if index < 0 {
			break
		}
		at = index
		if n == lines-1 && index+1 > start {
			start = index + 1
		}
	}
	return start
}
func textField(part map[string]any) map[string]any {
	if part["type"] == "text" {
		return part
	}
	if part["type"] == "resource" {
		resource := object(part["resource"])
		if _, ok := resource["text"].(string); ok {
			return resource
		}
	}
	return nil
}
func size(envelope map[string]any) (bytes, lines int) {
	for _, value := range array(envelope["content"]) {
		if field := textField(object(value)); field != nil {
			value := text(field["text"])
			bytes += len(value)
			lines += lineCount(value)
		}
	}
	if value, exists := envelope["structuredContent"]; exists {
		raw := encoded(value)
		bytes += len(raw)
		lines += lineCount(raw)
	}
	return
}
func notice(source map[string]any, tail bool, totalBytes, totalLines, shownBytes, shownLines, offset int) string {
	edge := "first"
	if tail {
		edge = "last"
	}
	return fmt.Sprintf("[Tool result truncated: showing the %s %d of %d lines (%d of %d bytes). Read the original result with read_tool_output using output=%s and offset=%d, then continue with next_offset]", edge, shownLines, totalLines, shownBytes, totalBytes, encoded(source), offset)
}
func bound(envelope map[string]any, source map[string]any, tail, plain bool) {
	bytes, lines := size(envelope)
	if bytes <= MaxBytes && lines <= MaxLines {
		return
	}
	prefix := false
	if value, exists := envelope["structuredContent"]; exists && !tail && plain && envelope["isError"] == nil {
		sum := blake3.Sum256([]byte(encoded(value)))
		prefix = fmt.Sprintf("%x", sum) == text(source["revision"])
	}
	if value, exists := envelope["structuredContent"]; exists {
		mirrored := false
		for _, content := range array(envelope["content"]) {
			if field := textField(object(content)); field != nil {
				raw := text(field["text"])
				if raw == encoded(value) || reflect.DeepEqual(decode(json.RawMessage(raw)), value) {
					mirrored = true
					break
				}
			}
		}
		if !mirrored {
			envelope["content"] = append(array(envelope["content"]), map[string]any{"type": "text", "text": encoded(value)})
		}
		delete(envelope, "structuredContent")
	}
	totalBytes, totalLines := size(envelope)
	if totalBytes <= MaxBytes && totalLines <= MaxLines {
		return
	}
	reserve := len(notice(source, tail, totalBytes, totalLines, totalBytes, totalLines, totalBytes))
	for _, content := range array(envelope["content"]) {
		if textField(object(content)) != nil {
			reserve++
		}
	}
	remainingBytes, remainingLines := MaxBytes-reserve, MaxLines-1
	if remainingBytes < 0 {
		remainingBytes = 0
	}
	contents := array(envelope["content"])
	indices := make([]int, len(contents))
	for i := range indices {
		indices[i] = i
	}
	if tail {
		for i, j := 0, len(indices)-1; i < j; i, j = i+1, j-1 {
			indices[i], indices[j] = indices[j], indices[i]
		}
	}
	shownBytes, shownLines := 0, 0
	cut := false
	for _, index := range indices {
		field := textField(object(contents[index]))
		if field == nil {
			continue
		}
		original := text(field["text"])
		if cut {
			field["text"] = ""
			continue
		}
		start, end := 0, len(original)
		if tail {
			start = tailStart(original, remainingBytes, remainingLines)
		} else {
			end = head(original, remainingBytes, remainingLines)
		}
		cut = end-start < len(original)
		shown := original[start:end]
		used := lineCount(shown)
		remainingBytes -= len(shown)
		remainingLines -= used
		shownBytes += len(shown)
		shownLines += used
		field["text"] = shown
	}
	kept := make([]any, 0, len(contents)+1)
	for _, content := range contents {
		field := textField(object(content))
		if field == nil || text(field["text"]) != "" {
			kept = append(kept, content)
		}
	}
	offset := 0
	if prefix {
		offset = shownBytes
	}
	kept = append(kept, map[string]any{"type": "text", "text": notice(source, tail, totalBytes, totalLines, shownBytes, shownLines, offset)})
	envelope["content"] = kept
}
func projectError(response *canonical.ToolResponse, source map[string]any) {
	value := object(decode(response.Result.Value))
	if value == nil {
		return
	}
	message := text(value["message"])
	data := ""
	if value["data"] != nil {
		data = encoded(value["data"])
	}
	if len(message)+len(data) <= MaxBytes && lineCount(message) <= MaxLines {
		return
	}
	envelope := map[string]any{"content": []any{map[string]any{"type": "text", "text": message + "\n" + data}}}
	bound(envelope, source, false, false)
	texts := []string{}
	for _, content := range array(envelope["content"]) {
		if field := textField(object(content)); field != nil {
			texts = append(texts, text(field["text"]))
		}
	}
	var joined strings.Builder
	for _, part := range texts {
		if joined.Len() > 0 && !strings.HasSuffix(joined.String(), "\n") {
			joined.WriteByte('\n')
		}
		joined.WriteString(part)
	}
	value["message"] = joined.String()
	delete(value, "data")
	response.Result.Value = json.RawMessage(encoded(value))
}

func headBytes(value string, maximum int) int {
	end := len(value)
	if end > maximum {
		end = maximum
	}
	for end > 0 && end < len(value) && !utf8.RuneStart(value[end]) {
		end--
	}
	return end
}
