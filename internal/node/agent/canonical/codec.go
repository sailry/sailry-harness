package canonical

import (
	"bytes"
	"encoding/json"
	"fmt"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var recordVariants = map[string]func() any{
	"message": func() any { return new(Message) }, "compaction": func() any { return new(Compaction) }, "configuration": func() any { return new(Configuration) }, "notice": func() any { return new(Notice) }, "operation": func() any { return new(Operation) },
}
var blockVariants = map[string]func() any{
	"text": func() any { return new(TextContent) }, "image": func() any { return new(AssetContent) }, "document": func() any { return new(AssetContent) }, "file": func() any { return new(FileContent) }, "tool_request": func() any { return new(ToolRequest) }, "tool_response": func() any { return new(ToolResponse) }, "thinking": func() any { return new(Thinking) }, "redacted_thinking": func() any { return new(RedactedThinking) },
}
var noticeVariants = map[string]func() any{"elicitation": func() any { return new(Elicitation) }, "elicitation_result": func() any { return new(ElicitationResult) }, "plan_review": func() any { return new(PlanReview) }, "plan_result": func() any { return new(PlanResult) }}
var operationVariants = map[string]func() any{"visibility": func() any { return new(Visibility) }, "tool_meta": func() any { return new(ToolMeta) }, "note": func() any { return new(Note) }}
var noteVariants = map[string]func() any{"empty_response": nil, "subagent_delivered": func() any { return new(SubagentDelivered) }, "subagents_cancelled": func() any { return new(SubagentsCancelled) }}

// joinObjects preserves serde's struct field order across flattened variants.
func joinObjects(left, right []byte) ([]byte, error) {
	if len(left) < 2 || len(right) < 2 || left[0] != '{' || right[0] != '{' {
		return nil, fmt.Errorf("flattened conversation payload must be an object")
	}
	if len(left) == 2 {
		return right, nil
	}
	if len(right) == 2 {
		return left, nil
	}
	out := append([]byte{}, left[:len(left)-1]...)
	out = append(out, ',')
	out = append(out, right[1:]...)
	return out, nil
}
func tagged(tag, kind string, data any, variants map[string]func() any) ([]byte, error) {
	if _, err := protocol.MarshalUnion(tag, "", kind, data, variants); err != nil {
		return nil, err
	}
	head, err := json.Marshal(map[string]string{tag: kind})
	if err != nil {
		return nil, err
	}
	if variants[kind] == nil {
		return head, nil
	}
	body, err := json.Marshal(data)
	if err != nil {
		return nil, err
	}
	return joinObjects(head, body)
}
func (r Record) MarshalJSON() ([]byte, error) {
	type plain Record
	b, err := json.Marshal(plain(r))
	if err != nil {
		return nil, err
	}
	v, err := tagged("kind", r.Kind, r.Data, recordVariants)
	if err != nil {
		return nil, err
	}
	return joinObjects(b, v)
}
func (r *Record) UnmarshalJSON(data []byte) error {
	type plain Record
	var value plain
	if err := protocol.DecodeObject(data, &value, []string{"id", "created", "turn"}, false); err != nil {
		return err
	}
	kind, payload, err := protocol.UnmarshalUnion(data, "kind", "", recordVariants, false)
	if err != nil {
		return err
	}
	*r = Record(value)
	r.Kind, r.Data = kind, payload
	return nil
}
func (b Block) MarshalJSON() ([]byte, error) {
	head, err := json.Marshal(struct {
		Index uint64 `json:"index"`
	}{b.Index})
	if err != nil {
		return nil, err
	}
	body, err := tagged("type", b.Kind, b.Data, blockVariants)
	if err != nil {
		return nil, err
	}
	return joinObjects(head, body)
}
func (b *Block) UnmarshalJSON(data []byte) error {
	var value struct {
		Index uint64 `json:"index"`
	}
	if err := protocol.DecodeObject(data, &value, []string{"index"}, false); err != nil {
		return err
	}
	kind, payload, err := protocol.UnmarshalUnion(data, "type", "", blockVariants, false)
	if err != nil {
		return err
	}
	*b = Block{Index: value.Index, Kind: kind, Data: payload}
	return nil
}
func (v Notice) MarshalJSON() ([]byte, error) {
	return tagged("notice", v.Kind, v.Data, noticeVariants)
}
func (v *Notice) UnmarshalJSON(data []byte) error {
	kind, payload, err := protocol.UnmarshalUnion(data, "notice", "", noticeVariants, false)
	if err != nil {
		return err
	}
	v.Kind, v.Data = kind, payload
	return nil
}
func (v Operation) MarshalJSON() ([]byte, error) {
	return tagged("operation", v.Kind, v.Data, operationVariants)
}
func (v *Operation) UnmarshalJSON(data []byte) error {
	kind, payload, err := protocol.UnmarshalUnion(data, "operation", "", operationVariants, false)
	if err != nil {
		return err
	}
	v.Kind, v.Data = kind, payload
	return nil
}
func (v OperationNote) MarshalJSON() ([]byte, error) {
	return tagged("note", v.Kind, v.Data, noteVariants)
}
func (v *OperationNote) UnmarshalJSON(data []byte) error {
	kind, payload, err := protocol.UnmarshalUnion(data, "note", "", noteVariants, false)
	if err != nil {
		return err
	}
	v.Kind, v.Data = kind, payload
	return nil
}
func (v Attachment) MarshalJSON() ([]byte, error) {
	return json.Marshal(struct {
		Type  string   `json:"type"`
		Asset AssetRef `json:"asset"`
	}{"inline", v.Asset})
}
func (v *Attachment) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Type  string   `json:"type"`
		Asset AssetRef `json:"asset"`
	}
	if err := protocol.DecodeObject(data, &decoded, []string{"type", "asset"}, false); err != nil {
		return err
	}
	if decoded.Type != "inline" {
		return fmt.Errorf("unknown attachment type %q", decoded.Type)
	}
	v.Asset = decoded.Asset
	return nil
}
func stringEnum(data []byte, choices ...string) (string, error) {
	var value string
	if err := json.Unmarshal(data, &value); err != nil {
		return "", err
	}
	for _, choice := range choices {
		if choice == value {
			return value, nil
		}
	}
	return "", fmt.Errorf("unknown conversation value %q", value)
}
func (v *Role) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "user", "assistant")
	*v = Role(s)
	return e
}
func (v *Finish) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "completed", "interrupted", "failed")
	*v = Finish(s)
	return e
}
func (v *ToolStatus) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "success", "error")
	*v = ToolStatus(s)
	return e
}
func (v *ToolFormat) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "json", "mcp", "mcp_error")
	if s == "json" {
		s = ""
	}
	*v = ToolFormat(s)
	return e
}
func (v ToolFormat) MarshalJSON() ([]byte, error) {
	s := string(v)
	if s == "" {
		s = "json"
	}
	return json.Marshal(s)
}
func (v *BinaryEncoding) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "bytes", "base64")
	*v = BinaryEncoding(s)
	return e
}
func (v *InputSource) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "user", "parent", "plugin", "notice")
	*v = InputSource(s)
	return e
}
func (v *Purpose) UnmarshalJSON(b []byte) error {
	s, e := stringEnum(b, "main", "compaction", "search")
	*v = Purpose(s)
	return e
}

// DecodeObject rejects explicit null for nonoptional scalar fields while the
// generated typed decoders below preserve default and optional fields.
func required(data []byte, target any, names []string) error {
	return protocol.DecodeObject(bytes.TrimSpace(data), target, names, false)
}
