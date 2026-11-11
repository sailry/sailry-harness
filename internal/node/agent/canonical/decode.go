package canonical

import (
	"bytes"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// decodeTrimmed decodes a value given directly to UnmarshalJSON. Unicode
// space around it, which is wider than JSON whitespace, has always been
// accepted there.
func decodeTrimmed(data []byte, target interface {
	DecodeJSON(*protocol.Decoder) error
}) error {
	return protocol.Decode(bytes.TrimSpace(data), target)
}

func (v *Provenance) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Provenance) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Provenance
	value := plain{}
	if err := decoder.Object(&value, []string{"author", "branch"}, false); err != nil {
		return err
	}
	*v = Provenance(value)
	return nil
}
func (v *Message) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Message) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Message
	value := plain{}
	if err := decoder.Object(&value, []string{"role"}, false); err != nil {
		return err
	}
	*v = Message(value)
	return nil
}
func (v *ResponseError) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ResponseError) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ResponseError
	value := plain{}
	if err := decoder.Object(&value, []string{"message"}, false); err != nil {
		return err
	}
	*v = ResponseError(value)
	return nil
}
func (v *Usage) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Usage) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Usage
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Usage(value)
	return nil
}
func (v *Sources) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Sources) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Sources
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Sources(value)
	return nil
}
func (v *Citation) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Citation) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Citation
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Citation(value)
	return nil
}
func (v *Grounding) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Grounding) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Grounding
	value := plain{}
	if err := decoder.Object(&value, []string{"uri"}, false); err != nil {
		return err
	}
	*v = Grounding(value)
	return nil
}
func (v *Compaction) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Compaction) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Compaction
	value := plain{}
	if err := decoder.Object(&value, []string{"boundary", "summary", "start", "end"}, false); err != nil {
		return err
	}
	*v = Compaction(value)
	return nil
}
func (v *CompactedFile) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *CompactedFile) DecodeJSON(decoder *protocol.Decoder) error {
	type plain CompactedFile
	value := plain{}
	if err := decoder.Object(&value, []string{"path"}, false); err != nil {
		return err
	}
	*v = CompactedFile(value)
	return nil
}
func (v *Configuration) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Configuration) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Configuration
	value := plain{}
	if err := decoder.Object(&value, []string{"context"}, false); err != nil {
		return err
	}
	*v = Configuration(value)
	return nil
}
func (v *ToolIdentity) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolIdentity) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolIdentity
	value := plain{}
	if err := decoder.Object(&value, []string{"name", "arguments"}, false); err != nil {
		return err
	}
	*v = ToolIdentity(value)
	return nil
}
func (v *Elicitation) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Elicitation) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Elicitation
	value := plain{}
	if err := decoder.Object(&value, []string{"parent", "request", "spec", "schema"}, false); err != nil {
		return err
	}
	*v = Elicitation(value)
	return nil
}
func (v *ElicitationResult) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ElicitationResult) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ElicitationResult
	value := plain{}
	if err := decoder.Object(&value, []string{"call", "response"}, false); err != nil {
		return err
	}
	*v = ElicitationResult(value)
	return nil
}
func (v *PlanReview) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *PlanReview) DecodeJSON(decoder *protocol.Decoder) error {
	type plain PlanReview
	value := plain{}
	if err := decoder.Object(&value, []string{"source"}, false); err != nil {
		return err
	}
	*v = PlanReview(value)
	return nil
}
func (v *Visibility) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Visibility) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Visibility
	value := plain{}
	if err := decoder.Object(&value, []string{"message", "user_visible", "agent_visible"}, false); err != nil {
		return err
	}
	*v = Visibility(value)
	return nil
}
func (v *ToolMeta) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolMeta) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolMeta
	value := plain{}
	if err := decoder.Object(&value, []string{"call", "patch"}, false); err != nil {
		return err
	}
	*v = ToolMeta(value)
	return nil
}
func (v *Note) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Note) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Note
	value := plain{}
	if err := decoder.Object(&value, []string{"message", "note"}, false); err != nil {
		return err
	}
	*v = Note(value)
	return nil
}
func (v *SubagentDelivered) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *SubagentDelivered) DecodeJSON(decoder *protocol.Decoder) error {
	type plain SubagentDelivered
	value := plain{}
	if err := decoder.Object(&value, []string{"session"}, false); err != nil {
		return err
	}
	*v = SubagentDelivered(value)
	return nil
}
func (v *SubagentsCancelled) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *SubagentsCancelled) DecodeJSON(decoder *protocol.Decoder) error {
	type plain SubagentsCancelled
	value := plain{}
	if err := decoder.Object(&value, []string{"sessions"}, false); err != nil {
		return err
	}
	*v = SubagentsCancelled(value)
	return nil
}
func (v *TextContent) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *TextContent) DecodeJSON(decoder *protocol.Decoder) error {
	type plain TextContent
	value := plain{}
	if err := decoder.Object(&value, []string{"text"}, false); err != nil {
		return err
	}
	*v = TextContent(value)
	return nil
}
func (v *AssetContent) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *AssetContent) DecodeJSON(decoder *protocol.Decoder) error {
	type plain AssetContent
	value := plain{}
	if err := decoder.Object(&value, []string{"asset"}, false); err != nil {
		return err
	}
	*v = AssetContent(value)
	return nil
}
func (v *FileContent) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *FileContent) DecodeJSON(decoder *protocol.Decoder) error {
	type plain FileContent
	value := plain{}
	if err := decoder.Object(&value, []string{"file"}, false); err != nil {
		return err
	}
	*v = FileContent(value)
	return nil
}
func (v *Thinking) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Thinking) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Thinking
	value := plain{}
	if err := decoder.Object(&value, []string{"text"}, false); err != nil {
		return err
	}
	*v = Thinking(value)
	return nil
}
func (v *RedactedThinking) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *RedactedThinking) DecodeJSON(decoder *protocol.Decoder) error {
	type plain RedactedThinking
	value := plain{}
	if err := decoder.Object(&value, []string{"data"}, false); err != nil {
		return err
	}
	*v = RedactedThinking(value)
	return nil
}
func (v *ToolRequest) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolRequest) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolRequest
	value := plain{}
	if err := decoder.Object(&value, []string{"id", "name", "arguments"}, false); err != nil {
		return err
	}
	*v = ToolRequest(value)
	return nil
}
func (v *ToolError) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolError) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolError
	value := plain{}
	if err := decoder.Object(&value, []string{"code", "message"}, false); err != nil {
		return err
	}
	*v = ToolError(value)
	return nil
}
func (v *ToolResponse) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolResponse) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolResponse
	value := plain{}
	if err := decoder.Object(&value, []string{"id", "name", "status", "result", "revision"}, false); err != nil {
		return err
	}
	*v = ToolResponse(value)
	return nil
}
func (v *AssetRef) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *AssetRef) DecodeJSON(decoder *protocol.Decoder) error {
	type plain AssetRef
	value := plain{}
	if err := decoder.Object(&value, []string{"digest", "mime", "size"}, false); err != nil {
		return err
	}
	*v = AssetRef(value)
	return nil
}
func (v *FileRef) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *FileRef) DecodeJSON(decoder *protocol.Decoder) error {
	type plain FileRef
	value := plain{}
	if err := decoder.Object(&value, []string{"uri"}, false); err != nil {
		return err
	}
	*v = FileRef(value)
	return nil
}
func (v *Payload) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Payload) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Payload
	value := plain{}
	if err := decoder.Object(&value, []string{"value"}, false); err != nil {
		return err
	}
	*v = Payload(value)
	return nil
}
func (v *BinarySlot) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *BinarySlot) DecodeJSON(decoder *protocol.Decoder) error {
	type plain BinarySlot
	value := plain{}
	if err := decoder.Object(&value, []string{"pointer", "asset", "encoding"}, false); err != nil {
		return err
	}
	*v = BinarySlot(value)
	return nil
}
func (v *Metadata) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Metadata) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Metadata
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Metadata(value)
	return nil
}
func (v *StateChanges) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *StateChanges) DecodeJSON(decoder *protocol.Decoder) error {
	type plain StateChanges
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = StateChanges(value)
	return nil
}
func (v *Input) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Input) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Input
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Input(value)
	return nil
}
func (v *Presentation) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Presentation) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Presentation
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Presentation(value)
	return nil
}
func (v *ToolFailure) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolFailure) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolFailure
	value := plain{}
	if err := decoder.Object(&value, []string{"index", "id", "name", "failure"}, false); err != nil {
		return err
	}
	*v = ToolFailure(value)
	return nil
}
func (v *ModelSource) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ModelSource) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ModelSource
	value := plain{}
	if err := decoder.Object(&value, []string{"id", "name", "api", "authentication", "model"}, false); err != nil {
		return err
	}
	*v = ModelSource(value)
	return nil
}
func (v *Execution) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Execution) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Execution
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Execution(value)
	return nil
}
func (v *Timing) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Timing) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Timing
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Timing(value)
	return nil
}
func (v *ToolStarted) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *ToolStarted) DecodeJSON(decoder *protocol.Decoder) error {
	type plain ToolStarted
	value := plain{}
	if err := decoder.Object(&value, []string{"name"}, false); err != nil {
		return err
	}
	*v = ToolStarted(value)
	return nil
}
func (v *Context) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Context) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Context
	value := plain{}
	if err := decoder.Object(&value, []string{"captured"}, false); err != nil {
		return err
	}
	*v = Context(value)
	return nil
}
func (v *Guide) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Guide) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Guide
	value := plain{}
	if err := decoder.Object(&value, []string{"path", "revision", "content"}, false); err != nil {
		return err
	}
	*v = Guide(value)
	return nil
}
func (v *Maintenance) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Maintenance) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Maintenance
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = Maintenance(value)
	return nil
}
func (v *Runtime) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Runtime) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Runtime
	value := plain{}
	if err := decoder.Object(&value, []string{"user_visible", "agent_visible", "output_limit_reached", "steer", "turn_context"}, false); err != nil {
		return err
	}
	*v = Runtime(value)
	return nil
}
func (v *OperationNotes) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *OperationNotes) DecodeJSON(decoder *protocol.Decoder) error {
	type plain OperationNotes
	value := plain{}
	if err := decoder.Object(&value, []string{}, false); err != nil {
		return err
	}
	*v = OperationNotes(value)
	return nil
}
func (v *PromptContext) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *PromptContext) DecodeJSON(decoder *protocol.Decoder) error {
	type plain PromptContext
	value := plain{}
	if err := decoder.Object(&value, []string{"state"}, false); err != nil {
		return err
	}
	*v = PromptContext(value)
	return nil
}
func (v *Inference) UnmarshalJSON(data []byte) error { return decodeTrimmed(data, v) }
func (v *Inference) DecodeJSON(decoder *protocol.Decoder) error {
	type plain Inference
	value := plain{}
	if err := decoder.Object(&value, []string{"provider", "requested_model"}, false); err != nil {
		return err
	}
	*v = Inference(value)
	return nil
}
