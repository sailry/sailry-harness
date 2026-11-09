package canonical

import (
	"encoding/json"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Metadata struct {
	Input        *Input        `json:"input,omitempty"`
	Presentation *Presentation `json:"presentation,omitempty"`
	Execution    *Execution    `json:"execution,omitempty"`
	Context      *Context      `json:"context,omitempty"`
	State        *StateChanges `json:"state,omitempty"`
	Compaction   *Maintenance  `json:"compaction,omitempty"`
	Runtime      *Runtime      `json:"runtime,omitempty"`
}
type StateChanges struct {
	Session     map[string]json.RawMessage `json:"session,omitempty"`
	Application map[string]json.RawMessage `json:"application,omitempty"`
	User        map[string]json.RawMessage `json:"user,omitempty"`
}
type Input struct {
	Turn           *protocol.TurnID                 `json:"turn,omitempty"`
	Source         *InputSource                     `json:"source,omitempty"`
	TextPart       *uint64                          `json:"text_part,omitempty"`
	CapabilityPart *uint64                          `json:"capability_part,omitempty"`
	NoticePart     *uint64                          `json:"notice_part,omitempty"`
	NoticeIDs      []string                         `json:"notice_ids,omitempty"`
	References     []protocol.ConversationReference `json:"references,omitempty"`
}
type InputSource string

const (
	InputUser   InputSource = "user"
	InputParent InputSource = "parent"
	InputPlugin InputSource = "plugin"
	InputNotice InputSource = "notice"
)

type Presentation struct {
	Groupings   map[string]protocol.ToolGrouping     `json:"groupings,omitempty"`
	Displays    map[string]protocol.ToolDisplay      `json:"displays,omitempty"`
	Tools       map[string]protocol.ToolPresentation `json:"tools,omitempty"`
	Delegations []string                             `json:"delegations,omitempty"`
	Failures    []ToolFailure                        `json:"failures,omitempty"`
}
type ToolFailure struct {
	Index   uint64               `json:"index"`
	ID      string               `json:"id"`
	Name    string               `json:"name"`
	Failure protocol.ToolFailure `json:"failure"`
	Message *string              `json:"message,omitempty"`
}
type ModelSource struct {
	ID             protocol.ProviderID           `json:"id"`
	Name           string                        `json:"name"`
	API            protocol.ConversationModelApi `json:"api"`
	Authentication protocol.Authentication       `json:"authentication"`
	Model          string                        `json:"model"`
}
type Execution struct {
	Purpose       *Purpose                                 `json:"purpose,omitempty"`
	Source        *ModelSource                             `json:"source,omitempty"`
	Media         *protocol.MediaKind                      `json:"media,omitempty"`
	Timing        *Timing                                  `json:"timing,omitempty"`
	Observation   *protocol.ConversationObservationRequest `json:"observation,omitempty"`
	Phases        *protocol.ConversationObservationPhases  `json:"phases,omitempty"`
	Retry         *protocol.ConversationRecoveryRetry      `json:"retry,omitempty"`
	ToolStarted   *ToolStarted                             `json:"tool_started,omitempty"`
	ContextTokens *uint64                                  `json:"context_tokens,omitempty"`
}
type Purpose string

const (
	PurposeMain       Purpose = "main"
	PurposeCompaction Purpose = "compaction"
	PurposeSearch     Purpose = "search"
)

type Timing struct {
	ElapsedUs    *uint64 `json:"elapsed_us,omitempty"`
	FirstTokenUs *uint64 `json:"first_token_us,omitempty"`
}
type ToolStarted struct {
	ID   *string `json:"id,omitempty"`
	Name string  `json:"name"`
}
type Context struct {
	Captured    bool             `json:"captured"`
	Title       *string          `json:"title,omitempty"`
	Policy      *string          `json:"policy,omitempty"`
	Environment *string          `json:"environment,omitempty"`
	Personal    *string          `json:"personal,omitempty"`
	Guides      map[string]Guide `json:"guides,omitempty"`
	LoadedTools *[]string        `json:"loaded_tools,omitempty"`
}
type Guide struct {
	Path     string `json:"path"`
	Revision string `json:"revision"`
	Content  string `json:"content"`
}
type Maintenance struct {
	LatestInstruction *string                                 `json:"latest_instruction,omitempty"`
	Failure           *protocol.ConversationCompactionFailure `json:"failure,omitempty"`
}
type Runtime struct {
	Created            *int64          `json:"created,omitempty"`
	After              *string         `json:"after,omitempty"`
	UserVisible        bool            `json:"user_visible"`
	AgentVisible       bool            `json:"agent_visible"`
	OutputLimitReached bool            `json:"output_limit_reached"`
	Steer              bool            `json:"steer"`
	TurnContext        bool            `json:"turn_context"`
	Inference          *Inference      `json:"inference,omitempty"`
	Notes              *OperationNotes `json:"notes,omitempty"`
	Logs               []string        `json:"logs,omitempty"`
}

func DefaultRuntime() Runtime { return Runtime{UserVisible: true, AgentVisible: true} }

type OperationNotes struct {
	EmptyResponse      bool           `json:"empty_response,omitempty"`
	DeliveredSubagent  *string        `json:"delivered_subagent,omitempty"`
	CancelledSubagents []string       `json:"cancelled_subagents,omitempty"`
	Context            *PromptContext `json:"context,omitempty"`
}
type PromptContext struct {
	State    string   `json:"state"`
	Retained []string `json:"retained,omitempty"`
}
type Inference struct {
	Provider        string  `json:"provider"`
	RequestedModel  string  `json:"requested_model"`
	ResolvedModel   *string `json:"resolved_model,omitempty"`
	ProviderSession *string `json:"provider_session,omitempty"`
}
