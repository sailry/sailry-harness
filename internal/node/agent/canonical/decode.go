package canonical

func (v *Provenance) UnmarshalJSON(data []byte) error {
	type plain Provenance
	value := plain{}
	if err := required(data, &value, []string{"author", "branch"}); err != nil {
		return err
	}
	*v = Provenance(value)
	return nil
}
func (v *Message) UnmarshalJSON(data []byte) error {
	type plain Message
	value := plain{}
	if err := required(data, &value, []string{"role"}); err != nil {
		return err
	}
	*v = Message(value)
	return nil
}
func (v *ResponseError) UnmarshalJSON(data []byte) error {
	type plain ResponseError
	value := plain{}
	if err := required(data, &value, []string{"message"}); err != nil {
		return err
	}
	*v = ResponseError(value)
	return nil
}
func (v *Usage) UnmarshalJSON(data []byte) error {
	type plain Usage
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Usage(value)
	return nil
}
func (v *Sources) UnmarshalJSON(data []byte) error {
	type plain Sources
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Sources(value)
	return nil
}
func (v *Citation) UnmarshalJSON(data []byte) error {
	type plain Citation
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Citation(value)
	return nil
}
func (v *Grounding) UnmarshalJSON(data []byte) error {
	type plain Grounding
	value := plain{}
	if err := required(data, &value, []string{"uri"}); err != nil {
		return err
	}
	*v = Grounding(value)
	return nil
}
func (v *Compaction) UnmarshalJSON(data []byte) error {
	type plain Compaction
	value := plain{}
	if err := required(data, &value, []string{"boundary", "summary", "start", "end"}); err != nil {
		return err
	}
	*v = Compaction(value)
	return nil
}
func (v *CompactedFile) UnmarshalJSON(data []byte) error {
	type plain CompactedFile
	value := plain{}
	if err := required(data, &value, []string{"path"}); err != nil {
		return err
	}
	*v = CompactedFile(value)
	return nil
}
func (v *Configuration) UnmarshalJSON(data []byte) error {
	type plain Configuration
	value := plain{}
	if err := required(data, &value, []string{"context"}); err != nil {
		return err
	}
	*v = Configuration(value)
	return nil
}
func (v *ToolIdentity) UnmarshalJSON(data []byte) error {
	type plain ToolIdentity
	value := plain{}
	if err := required(data, &value, []string{"name", "arguments"}); err != nil {
		return err
	}
	*v = ToolIdentity(value)
	return nil
}
func (v *Elicitation) UnmarshalJSON(data []byte) error {
	type plain Elicitation
	value := plain{}
	if err := required(data, &value, []string{"parent", "request", "spec", "schema"}); err != nil {
		return err
	}
	*v = Elicitation(value)
	return nil
}
func (v *ElicitationResult) UnmarshalJSON(data []byte) error {
	type plain ElicitationResult
	value := plain{}
	if err := required(data, &value, []string{"call", "response"}); err != nil {
		return err
	}
	*v = ElicitationResult(value)
	return nil
}
func (v *PlanReview) UnmarshalJSON(data []byte) error {
	type plain PlanReview
	value := plain{}
	if err := required(data, &value, []string{"source"}); err != nil {
		return err
	}
	*v = PlanReview(value)
	return nil
}
func (v *Visibility) UnmarshalJSON(data []byte) error {
	type plain Visibility
	value := plain{}
	if err := required(data, &value, []string{"message", "user_visible", "agent_visible"}); err != nil {
		return err
	}
	*v = Visibility(value)
	return nil
}
func (v *ToolMeta) UnmarshalJSON(data []byte) error {
	type plain ToolMeta
	value := plain{}
	if err := required(data, &value, []string{"call", "patch"}); err != nil {
		return err
	}
	*v = ToolMeta(value)
	return nil
}
func (v *Note) UnmarshalJSON(data []byte) error {
	type plain Note
	value := plain{}
	if err := required(data, &value, []string{"message", "note"}); err != nil {
		return err
	}
	*v = Note(value)
	return nil
}
func (v *SubagentDelivered) UnmarshalJSON(data []byte) error {
	type plain SubagentDelivered
	value := plain{}
	if err := required(data, &value, []string{"session"}); err != nil {
		return err
	}
	*v = SubagentDelivered(value)
	return nil
}
func (v *SubagentsCancelled) UnmarshalJSON(data []byte) error {
	type plain SubagentsCancelled
	value := plain{}
	if err := required(data, &value, []string{"sessions"}); err != nil {
		return err
	}
	*v = SubagentsCancelled(value)
	return nil
}
func (v *TextContent) UnmarshalJSON(data []byte) error {
	type plain TextContent
	value := plain{}
	if err := required(data, &value, []string{"text"}); err != nil {
		return err
	}
	*v = TextContent(value)
	return nil
}
func (v *AssetContent) UnmarshalJSON(data []byte) error {
	type plain AssetContent
	value := plain{}
	if err := required(data, &value, []string{"asset"}); err != nil {
		return err
	}
	*v = AssetContent(value)
	return nil
}
func (v *FileContent) UnmarshalJSON(data []byte) error {
	type plain FileContent
	value := plain{}
	if err := required(data, &value, []string{"file"}); err != nil {
		return err
	}
	*v = FileContent(value)
	return nil
}
func (v *Thinking) UnmarshalJSON(data []byte) error {
	type plain Thinking
	value := plain{}
	if err := required(data, &value, []string{"text"}); err != nil {
		return err
	}
	*v = Thinking(value)
	return nil
}
func (v *RedactedThinking) UnmarshalJSON(data []byte) error {
	type plain RedactedThinking
	value := plain{}
	if err := required(data, &value, []string{"data"}); err != nil {
		return err
	}
	*v = RedactedThinking(value)
	return nil
}
func (v *ToolRequest) UnmarshalJSON(data []byte) error {
	type plain ToolRequest
	value := plain{}
	if err := required(data, &value, []string{"id", "name", "arguments"}); err != nil {
		return err
	}
	*v = ToolRequest(value)
	return nil
}
func (v *ToolError) UnmarshalJSON(data []byte) error {
	type plain ToolError
	value := plain{}
	if err := required(data, &value, []string{"code", "message"}); err != nil {
		return err
	}
	*v = ToolError(value)
	return nil
}
func (v *ToolResponse) UnmarshalJSON(data []byte) error {
	type plain ToolResponse
	value := plain{}
	if err := required(data, &value, []string{"id", "name", "status", "result", "revision"}); err != nil {
		return err
	}
	*v = ToolResponse(value)
	return nil
}
func (v *AssetRef) UnmarshalJSON(data []byte) error {
	type plain AssetRef
	value := plain{}
	if err := required(data, &value, []string{"digest", "mime", "size"}); err != nil {
		return err
	}
	*v = AssetRef(value)
	return nil
}
func (v *FileRef) UnmarshalJSON(data []byte) error {
	type plain FileRef
	value := plain{}
	if err := required(data, &value, []string{"uri"}); err != nil {
		return err
	}
	*v = FileRef(value)
	return nil
}
func (v *Payload) UnmarshalJSON(data []byte) error {
	type plain Payload
	value := plain{}
	if err := required(data, &value, []string{"value"}); err != nil {
		return err
	}
	*v = Payload(value)
	return nil
}
func (v *BinarySlot) UnmarshalJSON(data []byte) error {
	type plain BinarySlot
	value := plain{}
	if err := required(data, &value, []string{"pointer", "asset", "encoding"}); err != nil {
		return err
	}
	*v = BinarySlot(value)
	return nil
}
func (v *Metadata) UnmarshalJSON(data []byte) error {
	type plain Metadata
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Metadata(value)
	return nil
}
func (v *StateChanges) UnmarshalJSON(data []byte) error {
	type plain StateChanges
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = StateChanges(value)
	return nil
}
func (v *Input) UnmarshalJSON(data []byte) error {
	type plain Input
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Input(value)
	return nil
}
func (v *Presentation) UnmarshalJSON(data []byte) error {
	type plain Presentation
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Presentation(value)
	return nil
}
func (v *ToolFailure) UnmarshalJSON(data []byte) error {
	type plain ToolFailure
	value := plain{}
	if err := required(data, &value, []string{"index", "id", "name", "failure"}); err != nil {
		return err
	}
	*v = ToolFailure(value)
	return nil
}
func (v *ModelSource) UnmarshalJSON(data []byte) error {
	type plain ModelSource
	value := plain{}
	if err := required(data, &value, []string{"id", "name", "api", "authentication", "model"}); err != nil {
		return err
	}
	*v = ModelSource(value)
	return nil
}
func (v *Execution) UnmarshalJSON(data []byte) error {
	type plain Execution
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Execution(value)
	return nil
}
func (v *Timing) UnmarshalJSON(data []byte) error {
	type plain Timing
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Timing(value)
	return nil
}
func (v *ToolStarted) UnmarshalJSON(data []byte) error {
	type plain ToolStarted
	value := plain{}
	if err := required(data, &value, []string{"name"}); err != nil {
		return err
	}
	*v = ToolStarted(value)
	return nil
}
func (v *Context) UnmarshalJSON(data []byte) error {
	type plain Context
	value := plain{}
	if err := required(data, &value, []string{"captured"}); err != nil {
		return err
	}
	*v = Context(value)
	return nil
}
func (v *Guide) UnmarshalJSON(data []byte) error {
	type plain Guide
	value := plain{}
	if err := required(data, &value, []string{"path", "revision", "content"}); err != nil {
		return err
	}
	*v = Guide(value)
	return nil
}
func (v *Maintenance) UnmarshalJSON(data []byte) error {
	type plain Maintenance
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = Maintenance(value)
	return nil
}
func (v *Runtime) UnmarshalJSON(data []byte) error {
	type plain Runtime
	value := plain{}
	if err := required(data, &value, []string{"user_visible", "agent_visible", "output_limit_reached", "steer", "turn_context"}); err != nil {
		return err
	}
	*v = Runtime(value)
	return nil
}
func (v *OperationNotes) UnmarshalJSON(data []byte) error {
	type plain OperationNotes
	value := plain{}
	if err := required(data, &value, []string{}); err != nil {
		return err
	}
	*v = OperationNotes(value)
	return nil
}
func (v *PromptContext) UnmarshalJSON(data []byte) error {
	type plain PromptContext
	value := plain{}
	if err := required(data, &value, []string{"state"}); err != nil {
		return err
	}
	*v = PromptContext(value)
	return nil
}
func (v *Inference) UnmarshalJSON(data []byte) error {
	type plain Inference
	value := plain{}
	if err := required(data, &value, []string{"provider", "requested_model"}); err != nil {
		return err
	}
	*v = Inference(value)
	return nil
}
