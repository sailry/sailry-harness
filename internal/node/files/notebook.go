package files

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"strings"
	"unicode/utf8"
)

type notebook struct {
	NBFormat uint32            `json:"nbformat"`
	Minor    uint32            `json:"nbformat_minor"`
	Metadata json.RawMessage   `json:"metadata"`
	Cells    []json.RawMessage `json:"cells"`
}

func parseNotebook(text string) (notebook, error) { return parseNotebookIDs(text, true) }
func parseNotebookIDs(text string, requireIDs bool) (notebook, error) {
	var n notebook
	var presence map[string]json.RawMessage
	if json.Unmarshal([]byte(text), &presence) != nil || presence["nbformat_minor"] == nil || string(presence["nbformat_minor"]) == "null" || !utf8.ValidString(text) {
		return n, invalid("notebook must use nbformat 4")
	}
	if len(text) > protocol.MAX_EDIT_BYTES || !nonnull(text) || json.Unmarshal([]byte(text), &n) != nil || n.NBFormat != 4 || n.Minor > 5 || len(n.Metadata) == 0 || n.Metadata[0] != '{' || n.Cells == nil {
		return n, invalid("notebook must use nbformat 4")
	}
	ids := map[string]bool{}
	for _, raw := range n.Cells {
		var c map[string]json.RawMessage
		if json.Unmarshal(raw, &c) != nil || len(c["metadata"]) == 0 || c["metadata"][0] != '{' {
			return n, invalid("invalid notebook cell")
		}
		var kind string
		if json.Unmarshal(c["cell_type"], &kind) != nil || (kind != "code" && kind != "markdown" && kind != "raw") {
			return n, invalid("invalid notebook cell")
		}
		if _, e := notebookSource(c["source"]); e != nil {
			return n, e
		}
		if kind == "code" {
			var outputs []json.RawMessage
			var count *uint64
			if json.Unmarshal(c["outputs"], &outputs) != nil || outputs == nil || len(c["execution_count"]) == 0 || json.Unmarshal(c["execution_count"], &count) != nil {
				return n, invalid("invalid notebook code outputs or execution count")
			}
		}
		if raw, exists := c["id"]; exists {
			var id string
			if json.Unmarshal(raw, &id) != nil || len(id) == 0 || len(id) > 64 || ids[id] || strings.ContainsFunc(id, func(r rune) bool {
				return !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '-' || r == '_')
			}) {
				return n, invalid("invalid or duplicate notebook cell ID")
			}
			ids[id] = true
		} else if requireIDs && n.Minor == 5 {
			return n, invalid("notebook cell ID is missing")
		}
	}
	return n, nil
}
func notebookSource(raw json.RawMessage) (string, error) {
	if len(raw) == 0 || string(raw) == "null" {
		return "", invalid("invalid notebook source")
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		if !nonnull(text) {
			return "", invalid("notebook cell source contains NUL")
		}
		return text, nil
	}
	var lines []string
	if json.Unmarshal(raw, &lines) != nil {
		return "", invalid("notebook cell source must be text or an array of text")
	}
	text = strings.Join(lines, "")
	if !nonnull(text) {
		return "", invalid("notebook cell source contains NUL")
	}
	return text, nil
}
func ReadNotebook(ctx context.Context, f *core.Files, input protocol.NotebookRead) (protocol.NotebookPage, error) {
	out := protocol.DefaultNotebookPage()
	if input.Limit == 0 {
		return out, invalid("notebook cell limit must be positive")
	}
	text, e := f.CompleteText(ctx, input.Path)
	if e != nil {
		return out, e
	}
	n, e := parseNotebook(text)
	if e != nil {
		return out, e
	}
	out.Path, out.Revision, out.Nbformat, out.NbformatMinor, out.Metadata, out.Total = input.Path, revision([]byte(text)), 4, n.Minor, n.Metadata, uint64(len(n.Cells))
	if len(out.Metadata) > protocol.MAX_FILE_BYTES {
		out.Metadata = json.RawMessage(`{}`)
		out.MetadataTruncated = true
	}
	size := len(out.Metadata) + 4096
	for index := input.Offset; index < uint64(len(n.Cells)); index++ {
		if e := ctx.Err(); e != nil {
			return out, e
		}
		raw := n.Cells[index]
		cell := protocol.NotebookCell{Index: index, Value: raw}
		if len(raw) > 512*1024 {
			var c map[string]json.RawMessage
			_ = json.Unmarshal(raw, &c)
			source, e := notebookSource(c["source"])
			if e != nil {
				return out, e
			}
			if len(source) > protocol.MAX_FILE_BYTES {
				source = source[:protocol.MAX_FILE_BYTES]
				for !utf8.ValidString(source) {
					source = source[:len(source)-1]
				}
			}
			value := map[string]any{"cell_type": c["cell_type"], "id": c["id"], "source": source, "metadata": map[string]any{}}
			cell.Value, _ = json.Marshal(value)
			cell.Truncated = true
		}
		rawCell, _ := json.Marshal(cell)
		if len(out.Cells) >= int(min(input.Limit, 100)) || size+len(rawCell) > 1024*1024 {
			next := index
			out.NextOffset = &next
			break
		}
		size += len(rawCell)
		out.Cells = append(out.Cells, cell)
	}
	return out, nil
}
func PrepareNotebook(before string, input protocol.NotebookEdit) (string, error) {
	if !validRevision(input.ExpectedRevision) {
		return "", invalid("invalid notebook revision")
	}
	if revision([]byte(before)) != input.ExpectedRevision {
		return "", conflict("notebook changed before editing")
	}
	if len(input.Edits) == 0 {
		return "", invalid("notebook edit count is out of range")
	}
	n, e := parseNotebook(before)
	if e != nil {
		return "", e
	}
	var document map[string]json.RawMessage
	_ = json.Unmarshal([]byte(before), &document)
	replacements := map[uint64]*string{}
	insertions := map[uint64][]json.RawMessage{}
	ids := map[string]bool{}
	for _, raw := range n.Cells {
		var c struct{ ID string }
		_ = json.Unmarshal(raw, &c)
		ids[c.ID] = true
	}
	for ordinal, edit := range input.Edits {
		switch edit.Kind {
		case "replace":
			v, ok := changePayload[protocol.NotebookChangeReplacePayload](edit.Data)
			if !ok || v.Index >= uint64(len(n.Cells)) || !nonnull(v.Source) {
				return "", invalid("notebook edit indices overlap or are out of range")
			}
			if _, exists := replacements[v.Index]; exists {
				return "", invalid("notebook edit indices overlap or are out of range")
			}
			source := v.Source
			replacements[v.Index] = &source
		case "delete":
			v, ok := changePayload[protocol.NotebookChangeDeletePayload](edit.Data)
			if !ok || v.Index >= uint64(len(n.Cells)) {
				return "", invalid("notebook edit indices overlap or are out of range")
			}
			if _, exists := replacements[v.Index]; exists {
				return "", invalid("notebook edit indices overlap or are out of range")
			}
			replacements[v.Index] = nil
		case "insert":
			v, ok := changePayload[protocol.NotebookChangeInsertPayload](edit.Data)
			if !ok || v.Index > uint64(len(n.Cells)) || !nonnull(v.Source) || (v.CellType != "code" && v.CellType != "markdown" && v.CellType != "raw") {
				return "", invalid("notebook insertion index is out of range")
			}
			id := ""
			for nonce := 0; ; nonce++ {
				id = revision([]byte(fmt.Sprintf("%s:%d:%d", input.ExpectedRevision, ordinal, nonce)))
				if !ids[id] {
					ids[id] = true
					break
				}
			}
			c := map[string]any{"id": id, "cell_type": v.CellType, "source": v.Source, "metadata": map[string]any{}}
			if v.CellType == "code" {
				c["outputs"] = []any{}
				c["execution_count"] = nil
			}
			raw, _ := json.Marshal(c)
			insertions[v.Index] = append(insertions[v.Index], raw)
		default:
			return "", invalid("invalid notebook edit kind")
		}
	}
	cells := []json.RawMessage{}
	for index := uint64(0); index <= uint64(len(n.Cells)); index++ {
		cells = append(cells, insertions[index]...)
		if index == uint64(len(n.Cells)) {
			break
		}
		raw := n.Cells[index]
		if source, changed := replacements[index]; changed {
			if source == nil {
				continue
			}
			var c map[string]json.RawMessage
			_ = json.Unmarshal(raw, &c)
			c["source"], _ = json.Marshal(*source)
			var kind string
			_ = json.Unmarshal(c["cell_type"], &kind)
			if kind == "code" {
				c["outputs"] = json.RawMessage(`[]`)
				c["execution_count"] = json.RawMessage(`null`)
			}
			raw, _ = json.Marshal(c)
		}
		cells = append(cells, raw)
	}
	document["cells"], _ = json.Marshal(cells)
	raw, e := json.MarshalIndent(document, "", "  ")
	if e != nil {
		return "", e
	}
	raw = append(raw, '\n')
	if len(raw) > protocol.MAX_EDIT_BYTES {
		return "", invalid("edited notebook exceeds the text limit")
	}
	return string(raw), nil
}

// PrepareCreation assigns stable IDs to generated v4.5 notebook cells before approval.
func PrepareCreation(path, text string) (string, error) {
	if !strings.HasSuffix(strings.ToLower(path), ".ipynb") {
		return text, nil
	}
	n, err := parseNotebookIDs(text, false)
	if err != nil {
		return "", err
	}
	if n.Minor != 5 {
		return text, nil
	}
	ids := map[string]bool{}
	for _, raw := range n.Cells {
		var cell map[string]json.RawMessage
		_ = json.Unmarshal(raw, &cell)
		var id string
		if json.Unmarshal(cell["id"], &id) == nil {
			ids[id] = true
		}
	}
	changed := false
	for index, raw := range n.Cells {
		var cell map[string]json.RawMessage
		_ = json.Unmarshal(raw, &cell)
		if _, ok := cell["id"]; ok {
			continue
		}
		var id string
		for nonce := 0; ; nonce++ {
			id = revision([]byte(fmt.Sprintf("%s:%d:%d", revision([]byte(text)), index, nonce)))
			if !ids[id] {
				ids[id] = true
				break
			}
		}
		cell["id"], _ = json.Marshal(id)
		n.Cells[index], _ = json.Marshal(cell)
		changed = true
	}
	if !changed {
		return text, nil
	}
	var document map[string]json.RawMessage
	_ = json.Unmarshal([]byte(text), &document)
	document["cells"], _ = json.Marshal(n.Cells)
	raw, err := json.MarshalIndent(document, "", "  ")
	if err != nil {
		return "", err
	}
	out := string(raw) + "\n"
	_, err = parseNotebook(out)
	return out, err
}
