package canonical

import "encoding/json"

type Block struct {
	Index uint64 `json:"index"`
	Kind  string `json:"-"`
	Data  any    `json:"-"`
}
type TextContent struct {
	Text string `json:"text"`
}
type AssetContent struct {
	Asset AssetRef `json:"asset"`
}
type FileContent struct {
	File FileRef `json:"file"`
}
type Thinking struct {
	Text      string  `json:"text"`
	Signature *string `json:"signature"`
}
type RedactedThinking struct {
	Data   []string  `json:"data"`
	Before []*uint64 `json:"before"`
}
type ToolRequest struct {
	ID        string                      `json:"id"`
	Name      string                      `json:"name"`
	Arguments json.RawMessage             `json:"arguments"`
	Provider  *map[string]json.RawMessage `json:"provider,omitempty"`
	ToolMeta  json.RawMessage             `json:"tool_meta,omitempty"`
	Error     *ToolError                  `json:"error,omitempty"`
}
type ToolError struct {
	Code    int64           `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}
type ToolResponse struct {
	ID          string                      `json:"id"`
	Name        string                      `json:"name"`
	Status      ToolStatus                  `json:"status"`
	Result      Payload                     `json:"result"`
	Revision    string                      `json:"revision"`
	Attachments []Attachment                `json:"attachments,omitempty"`
	Provider    *map[string]json.RawMessage `json:"provider,omitempty"`
	Format      ToolFormat                  `json:"format,omitempty"`
	Meta        *map[string]json.RawMessage `json:"meta,omitempty"`
}
type ToolStatus string

const (
	ToolSuccess     ToolStatus = "success"
	ToolErrorStatus ToolStatus = "error"
)

type ToolFormat string

const (
	JSON     ToolFormat = ""
	MCP      ToolFormat = "mcp"
	MCPError ToolFormat = "mcp_error"
)

type Attachment struct {
	Asset AssetRef `json:"asset"`
}
type AssetRef struct {
	Digest      string          `json:"digest"`
	Mime        string          `json:"mime"`
	Size        uint64          `json:"size"`
	URI         *string         `json:"uri,omitempty"`
	Annotations json.RawMessage `json:"annotations,omitempty"`
}
type FileRef struct {
	URI  string  `json:"uri"`
	Mime *string `json:"mime,omitempty"`
}
type Payload struct {
	Value  json.RawMessage `json:"value"`
	Assets []BinarySlot    `json:"assets,omitempty"`
}
type BinarySlot struct {
	Pointer  string         `json:"pointer"`
	Asset    AssetRef       `json:"asset"`
	Encoding BinaryEncoding `json:"encoding"`
}
type BinaryEncoding string

const (
	Bytes  BinaryEncoding = "bytes"
	Base64 BinaryEncoding = "base64"
)
