// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationExecution string

const (
	ConversationExecutionModel ConversationExecution = "model"
	ConversationExecutionTeam  ConversationExecution = "team"
)

func (value ConversationExecution) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *ConversationExecution) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *ConversationExecution) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch ConversationExecution(decoded) {
	case ConversationExecutionModel, ConversationExecutionTeam:
		*value = ConversationExecution(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value ConversationExecution) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationExecution) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationInput struct {
	Text        string                  `json:"text"`
	Attachments []AttachmentID          `json:"attachments"`
	References  []ConversationReference `json:"references,omitempty"`
}

func DefaultConversationInput() ConversationInput {
	return ConversationInput{
		Text:        "",
		Attachments: []AttachmentID{},
		References:  []ConversationReference{},
	}
}
func (value *ConversationInput) UnmarshalJSON(data []byte) error {
	type plain ConversationInput
	decoded := plain{}
	decoded.References = []ConversationReference{}
	if err := DecodeObject(data, &decoded, []string{"text", "attachments"}, false); err != nil {
		return err
	}
	*value = ConversationInput(decoded)
	return nil
}
func (value ConversationInput) MarshalJSON() ([]byte, error) {
	type plain ConversationInput
	encoded := plain(value)
	encoded.Attachments = normalizeCollections(encoded.Attachments)
	encoded.References = normalizeCollections(encoded.References)
	return json.Marshal(encoded)
}

type ConversationStart struct {
	Project     *ProjectID               `json:"project"`
	Worktree    *WorktreeID              `json:"worktree"`
	Config      *SessionConfig           `json:"config"`
	Title       string                   `json:"title"`
	Message     ConversationInput        `json:"message"`
	Execution   ConversationExecution    `json:"execution"`
	Association *ConversationAssociation `json:"association,omitempty"`
	Team        *TeamSelection           `json:"team,omitempty"`
	Models      map[RoleID]RoleModel     `json:"models,omitempty"`
}

func DefaultConversationStart() ConversationStart {
	return ConversationStart{
		Project:     nil,
		Worktree:    nil,
		Config:      nil,
		Title:       "",
		Message:     DefaultConversationInput(),
		Execution:   ConversationExecutionTeam,
		Association: nil,
		Team:        nil,
		Models:      map[RoleID]RoleModel{},
	}
}
func (value *ConversationStart) UnmarshalJSON(data []byte) error {
	type plain ConversationStart
	decoded := plain{}
	decoded.Execution = ConversationExecutionTeam
	decoded.Association = nil
	decoded.Team = nil
	decoded.Models = map[RoleID]RoleModel{}
	if err := DecodeObject(data, &decoded, []string{"title", "message"}, true); err != nil {
		return err
	}
	*value = ConversationStart(decoded)
	return nil
}
func (value ConversationStart) MarshalJSON() ([]byte, error) {
	type plain ConversationStart
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Execution == ConversationExecutionTeam {
		delete(fields, "execution")
	}
	return json.Marshal(fields)
}

type ConversationModelApi string

const (
	ConversationModelApiChatCompletions ConversationModelApi = "chat_completions"
	ConversationModelApiResponses       ConversationModelApi = "responses"
	ConversationModelApiAnthropic       ConversationModelApi = "anthropic"
	ConversationModelApiGemini          ConversationModelApi = "gemini"
	ConversationModelApiDeepSeek        ConversationModelApi = "deepseek"
	ConversationModelApiOpenCodeGo      ConversationModelApi = "open_code_go"
	ConversationModelApiOpenCodeZen     ConversationModelApi = "open_code_zen"
	ConversationModelApiAzureOpenAi     ConversationModelApi = "azure_open_ai"
	ConversationModelApiAzureAi         ConversationModelApi = "azure_ai"
	ConversationModelApiPlugin          ConversationModelApi = "plugin"
)

func (value ConversationModelApi) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationModelApi) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationModelApi(decoded) {
	case ConversationModelApiChatCompletions, ConversationModelApiResponses, ConversationModelApiAnthropic, ConversationModelApiGemini, ConversationModelApiDeepSeek, ConversationModelApiOpenCodeGo, ConversationModelApiOpenCodeZen, ConversationModelApiAzureOpenAi, ConversationModelApiAzureAi, ConversationModelApiPlugin:
		*value = ConversationModelApi(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationModelApi) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationModelApi) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationProvider struct {
	ID                ProviderID                     `json:"id"`
	Revision          uint64                         `json:"revision"`
	Preset            *ConversationPresetsPresetRef  `json:"preset,omitempty"`
	Binding           **ConversationPresetsBinding   `json:"binding,omitempty"`
	Consent           **ConversationConsentApproval  `json:"consent,omitempty"`
	Name              string                         `json:"name"`
	Api               ConversationModelApi           `json:"api"`
	Authentication    Authentication                 `json:"authentication"`
	Endpoint          string                         `json:"endpoint"`
	Options           *ConversationConnectionOptions `json:"options,omitempty"`
	Oauth             *ConversationOauthOptions      `json:"oauth,omitempty"`
	Enabled           bool                           `json:"enabled"`
	Models            []ConversationModel            `json:"models"`
	DefaultModel      string                         `json:"default_model"`
	RequestMaxRetries *uint64                        `json:"request_max_retries,omitempty"`
	StreamMaxRetries  *uint64                        `json:"stream_max_retries,omitempty"`
	Credential        *CredentialRef                 `json:"credential"`
}

func DefaultConversationProvider() ConversationProvider {
	return ConversationProvider{
		ID:                ProviderID{},
		Revision:          0,
		Preset:            nil,
		Binding:           nil,
		Consent:           nil,
		Name:              "",
		Api:               ConversationModelApi(""),
		Authentication:    Authentication(""),
		Endpoint:          "",
		Options:           nil,
		Oauth:             nil,
		Enabled:           false,
		Models:            []ConversationModel{},
		DefaultModel:      "",
		RequestMaxRetries: nil,
		StreamMaxRetries:  nil,
		Credential:        nil,
	}
}
func (value *ConversationProvider) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationProvider) DecodeJSON(decoder *Decoder) error {
	type plain ConversationProvider
	decoded := plain{}
	decoded.Preset = nil
	decoded.Binding = nil
	decoded.Consent = nil
	decoded.Options = nil
	decoded.Oauth = nil
	decoded.Models = []ConversationModel{}
	decoded.RequestMaxRetries = nil
	decoded.StreamMaxRetries = nil
	if err := decoder.Object(&decoded, []string{"id", "revision", "name", "api", "authentication", "endpoint", "enabled", "default_model"}, false); err != nil {
		return err
	}
	*value = ConversationProvider(decoded)
	return nil
}
func (value ConversationProvider) MarshalJSON() ([]byte, error) {
	type plain ConversationProvider
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type ConversationModel struct {
	ID               string                             `json:"id"`
	DisplayName      *string                            `json:"display_name,omitempty"`
	Context          uint32                             `json:"context"`
	Output           uint32                             `json:"output"`
	CompactionTokens *uint32                            `json:"compaction_tokens,omitempty"`
	Capabilities     *ConversationDiscoveryCapabilities `json:"capabilities,omitempty"`
	Vision           bool                               `json:"vision"`
	Tools            bool                               `json:"tools"`
	Reasoning        bool                               `json:"reasoning"`
	Generates        []MediaGeneration                  `json:"generates"`
	// CustomCapabilities marks vision, tools, reasoning and generates as chosen
	// by the user, so catalog completion and discovery keep them.
	CustomCapabilities bool     `json:"custom_capabilities,omitempty"`
	Efforts            []Effort `json:"efforts"`
	CustomEfforts      bool     `json:"custom_efforts"`
	DefaultEffort      Effort   `json:"default_effort"`
}

func DefaultConversationModel() ConversationModel {
	return ConversationModel{
		ID:               "",
		DisplayName:      nil,
		Context:          0,
		Output:           0,
		CompactionTokens: nil,
		Capabilities:     nil,
		Vision:           false,
		Tools:            false,
		Reasoning:        false,
		Generates:        []MediaGeneration{},
		Efforts:          []Effort{},
		CustomEfforts:    false,
		DefaultEffort:    Effort{Kind: "default"},
	}
}
func (value *ConversationModel) UnmarshalJSON(data []byte) error {
	type plain ConversationModel
	decoded := plain{}
	decoded.DisplayName = nil
	decoded.CompactionTokens = nil
	decoded.Capabilities = nil
	decoded.Vision = false
	decoded.Tools = false
	decoded.Reasoning = false
	decoded.Generates = []MediaGeneration{}
	decoded.Efforts = []Effort{}
	decoded.CustomEfforts = false
	decoded.DefaultEffort = Effort{Kind: "default"}
	if err := DecodeObject(data, &decoded, []string{"id", "context", "output"}, false); err != nil {
		return err
	}
	*value = ConversationModel(decoded)
	return nil
}
func (value ConversationModel) MarshalJSON() ([]byte, error) {
	type plain ConversationModel
	encoded := plain(value)
	encoded.Generates = normalizeCollections(encoded.Generates)
	encoded.Efforts = normalizeCollections(encoded.Efforts)
	return json.Marshal(encoded)
}

type ConversationStatus string

const (
	ConversationStatusQueued      ConversationStatus = "queued"
	ConversationStatusRunning     ConversationStatus = "running"
	ConversationStatusStopping    ConversationStatus = "stopping"
	ConversationStatusCompleted   ConversationStatus = "completed"
	ConversationStatusContinued   ConversationStatus = "continued"
	ConversationStatusCancelled   ConversationStatus = "cancelled"
	ConversationStatusInterrupted ConversationStatus = "interrupted"
	ConversationStatusFailed      ConversationStatus = "failed"
)

func (value ConversationStatus) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *ConversationStatus) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *ConversationStatus) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch ConversationStatus(decoded) {
	case ConversationStatusQueued, ConversationStatusRunning, ConversationStatusStopping, ConversationStatusCompleted, ConversationStatusContinued, ConversationStatusCancelled, ConversationStatusInterrupted, ConversationStatusFailed:
		*value = ConversationStatus(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value ConversationStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationRunKind string

const (
	ConversationRunKindTask       ConversationRunKind = "task"
	ConversationRunKindCompaction ConversationRunKind = "compaction"
)

func (value ConversationRunKind) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *ConversationRunKind) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *ConversationRunKind) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch ConversationRunKind(decoded) {
	case ConversationRunKindTask, ConversationRunKindCompaction:
		*value = ConversationRunKind(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value ConversationRunKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationRunKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationRun struct {
	Worktree     WorktreeID                     `json:"worktree"`
	Turn         TurnID                         `json:"turn"`
	Kind         ConversationRunKind            `json:"kind"`
	Session      SessionID                      `json:"session"`
	Sequence     uint64                         `json:"sequence"`
	Revision     uint64                         `json:"revision"`
	Status       ConversationStatus             `json:"status"`
	Error        *Fault                         `json:"error"`
	StartedMs    *int64                         `json:"started_ms"`
	FinishedMs   *int64                         `json:"finished_ms"`
	Origin       *SessionID                     `json:"origin,omitempty"`
	Continuation *ConversationContinuation      `json:"continuation,omitempty"`
	Guidance     *TurnID                        `json:"guidance,omitempty"`
	Joined       *TurnID                        `json:"joined,omitempty"`
	Notice       bool                           `json:"notice"`
	Notices      []ConversationBackgroundNotice `json:"notices"`
	Input        *ConversationSentInput         `json:"input,omitempty"`
}

func DefaultConversationRun() ConversationRun {
	return ConversationRun{
		Worktree:     WorktreeID{},
		Turn:         TurnID{},
		Kind:         ConversationRunKind(""),
		Session:      SessionID{},
		Sequence:     0,
		Revision:     0,
		Status:       ConversationStatus(""),
		Error:        nil,
		StartedMs:    nil,
		FinishedMs:   nil,
		Origin:       nil,
		Continuation: nil,
		Guidance:     nil,
		Joined:       nil,
		Notice:       false,
		Notices:      []ConversationBackgroundNotice{},
		Input:        nil,
	}
}
func (value *ConversationRun) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationRun) DecodeJSON(decoder *Decoder) error {
	type plain ConversationRun
	decoded := plain{}
	decoded.Continuation = nil
	decoded.Guidance = nil
	decoded.Joined = nil
	decoded.Notice = false
	decoded.Notices = []ConversationBackgroundNotice{}
	decoded.Input = nil
	if err := decoder.Object(&decoded, []string{"worktree", "turn", "kind", "session", "sequence", "revision", "status"}, false); err != nil {
		return err
	}
	*value = ConversationRun(decoded)
	return nil
}
func (value ConversationRun) MarshalJSON() ([]byte, error) {
	type plain ConversationRun
	encoded := plain(value)
	encoded.Notices = normalizeCollections(encoded.Notices)
	return json.Marshal(encoded)
}

type ConversationSentInput struct {
	Message     ConversationInput `json:"message"`
	Attachments []Attachment      `json:"attachments"`
}

func DefaultConversationSentInput() ConversationSentInput {
	return ConversationSentInput{
		Message:     DefaultConversationInput(),
		Attachments: []Attachment{},
	}
}
func (value *ConversationSentInput) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationSentInput) DecodeJSON(decoder *Decoder) error {
	type plain ConversationSentInput
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"message", "attachments"}, false); err != nil {
		return err
	}
	*value = ConversationSentInput(decoded)
	return nil
}
func (value ConversationSentInput) MarshalJSON() ([]byte, error) {
	type plain ConversationSentInput
	encoded := plain(value)
	encoded.Attachments = normalizeCollections(encoded.Attachments)
	return json.Marshal(encoded)
}

// ConversationContinuation marks a turn a package started with continue_turn.
// Marker is the localized label controllers show in place of its input; Kind
// is the declared marker's id when the package named one; Limit is the
// automatic turn limit the package declared for the session; User marks a
// continuation the user asked for, which starts a new automatic chain.
type ConversationContinuation struct {
	Package PluginReference          `json:"package"`
	Marker  *PluginDesktopNavigation `json:"marker"`
	Kind    *string                  `json:"kind,omitempty"`
	Limit   *uint32                  `json:"limit,omitempty"`
	User    bool                     `json:"user,omitempty"`
}

func DefaultConversationContinuation() ConversationContinuation {
	return ConversationContinuation{
		Package: DefaultPluginReference(),
		Marker:  nil,
		Kind:    nil,
		Limit:   nil,
		User:    false,
	}
}
func (value *ConversationContinuation) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationContinuation) DecodeJSON(decoder *Decoder) error {
	type plain ConversationContinuation
	decoded := plain{}
	decoded.Kind = nil
	decoded.Limit = nil
	if err := decoder.Object(&decoded, []string{"package"}, false); err != nil {
		return err
	}
	*value = ConversationContinuation(decoded)
	return nil
}
func (value ConversationContinuation) MarshalJSON() ([]byte, error) {
	type plain ConversationContinuation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationChild struct {
	Run    ConversationRun `json:"run"`
	Origin Delegation      `json:"origin"`
	Name   *string         `json:"name"`
}

func DefaultConversationChild() ConversationChild {
	return ConversationChild{
		Run:    DefaultConversationRun(),
		Origin: DefaultDelegation(),
		Name:   nil,
	}
}
func (value *ConversationChild) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationChild) DecodeJSON(decoder *Decoder) error {
	type plain ConversationChild
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"run", "origin"}, false); err != nil {
		return err
	}
	*value = ConversationChild(decoded)
	return nil
}
func (value ConversationChild) MarshalJSON() ([]byte, error) {
	type plain ConversationChild
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationFork struct {
	Session SessionID `json:"session"`
	Through TurnID    `json:"through"`
}

func DefaultConversationFork() ConversationFork {
	return ConversationFork{
		Session: SessionID{},
		Through: TurnID{},
	}
}
func (value *ConversationFork) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationFork) DecodeJSON(decoder *Decoder) error {
	type plain ConversationFork
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"session", "through"}, false); err != nil {
		return err
	}
	*value = ConversationFork(decoded)
	return nil
}
func (value ConversationFork) MarshalJSON() ([]byte, error) {
	type plain ConversationFork
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationRewind struct {
	Session  SessionID `json:"session"`
	Revision uint64    `json:"revision"`
	Through  *TurnID   `json:"through"`
	Backup   *Session  `json:"backup,omitempty"`
}

func DefaultConversationRewind() ConversationRewind {
	return ConversationRewind{
		Session:  SessionID{},
		Revision: 0,
		Through:  nil,
		Backup:   nil,
	}
}
func (value *ConversationRewind) UnmarshalJSON(data []byte) error {
	type plain ConversationRewind
	decoded := plain{}
	decoded.Backup = nil
	if err := DecodeObject(data, &decoded, []string{"session", "revision"}, false); err != nil {
		return err
	}
	*value = ConversationRewind(decoded)
	return nil
}
func (value ConversationRewind) MarshalJSON() ([]byte, error) {
	type plain ConversationRewind
	encoded := plain(value)
	return json.Marshal(encoded)
}

// ConversationQueuePause says why a queue is paused: the user paused it,
// the user stopped a turn, a turn lost its owner, or a turn ended before its
// input reached history.
type ConversationQueuePause string

const (
	ConversationQueuePauseUser        ConversationQueuePause = "user"
	ConversationQueuePauseStopped     ConversationQueuePause = "stopped"
	ConversationQueuePauseInterrupted ConversationQueuePause = "interrupted"
	ConversationQueuePauseUnrecorded  ConversationQueuePause = "unrecorded"
)

func (value ConversationQueuePause) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationQueuePause) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationQueuePause(decoded) {
	case ConversationQueuePauseUser, ConversationQueuePauseStopped, ConversationQueuePauseInterrupted, ConversationQueuePauseUnrecorded:
		*value = ConversationQueuePause(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}

type ConversationQueue struct {
	Revision    uint64                  `json:"revision"`
	Paused      bool                    `json:"paused"`
	PauseReason *ConversationQueuePause `json:"pause_reason,omitempty"`
	Items       []ConversationPending   `json:"items"`
}

func DefaultConversationQueue() ConversationQueue {
	return ConversationQueue{
		Revision:    0,
		Paused:      false,
		PauseReason: nil,
		Items:       []ConversationPending{},
	}
}
func (value *ConversationQueue) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationQueue) DecodeJSON(decoder *Decoder) error {
	type plain ConversationQueue
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"revision", "paused", "items"}, false); err != nil {
		return err
	}
	*value = ConversationQueue(decoded)
	return nil
}
func (value ConversationQueue) MarshalJSON() ([]byte, error) {
	type plain ConversationQueue
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type ConversationPending struct {
	Turn           TurnID              `json:"turn"`
	Notice         bool                `json:"notice"`
	Request        *RequestID          `json:"request,omitempty"`
	Kind           ConversationRunKind `json:"kind"`
	Revision       uint64              `json:"revision"`
	ConfigRevision uint64              `json:"config_revision"`
	Ready          bool                `json:"ready"`
	Guidance       *TurnID             `json:"guidance"`
	Preview        string              `json:"preview"`
	Truncated      bool                `json:"truncated"`
	Attachments    []Attachment        `json:"attachments"`
}

func DefaultConversationPending() ConversationPending {
	return ConversationPending{
		Turn:           TurnID{},
		Notice:         false,
		Request:        nil,
		Kind:           ConversationRunKind(""),
		Revision:       0,
		ConfigRevision: 0,
		Ready:          false,
		Guidance:       nil,
		Preview:        "",
		Truncated:      false,
		Attachments:    []Attachment{},
	}
}
func (value *ConversationPending) UnmarshalJSON(data []byte) error {
	type plain ConversationPending
	decoded := plain{}
	decoded.Notice = false
	decoded.Request = nil
	if err := DecodeObject(data, &decoded, []string{"turn", "kind", "revision", "config_revision", "ready", "preview", "truncated", "attachments"}, false); err != nil {
		return err
	}
	*value = ConversationPending(decoded)
	return nil
}
func (value ConversationPending) MarshalJSON() ([]byte, error) {
	type plain ConversationPending
	encoded := plain(value)
	encoded.Attachments = normalizeCollections(encoded.Attachments)
	return json.Marshal(encoded)
}

type ConversationQueuedMessage struct {
	Turn     QueuedTurn        `json:"turn"`
	Revision uint64            `json:"revision"`
	Message  ConversationInput `json:"message"`
}

func DefaultConversationQueuedMessage() ConversationQueuedMessage {
	return ConversationQueuedMessage{
		Turn:     DefaultQueuedTurn(),
		Revision: 0,
		Message:  DefaultConversationInput(),
	}
}
func (value *ConversationQueuedMessage) UnmarshalJSON(data []byte) error {
	type plain ConversationQueuedMessage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "revision", "message"}, false); err != nil {
		return err
	}
	*value = ConversationQueuedMessage(decoded)
	return nil
}
func (value ConversationQueuedMessage) MarshalJSON() ([]byte, error) {
	type plain ConversationQueuedMessage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationQueueUpdate struct {
	Session SessionID         `json:"session"`
	Queue   ConversationQueue `json:"queue"`
	Runs    []ConversationRun `json:"runs"`
}

func DefaultConversationQueueUpdate() ConversationQueueUpdate {
	return ConversationQueueUpdate{
		Session: SessionID{},
		Queue:   DefaultConversationQueue(),
		Runs:    []ConversationRun{},
	}
}
func (value *ConversationQueueUpdate) UnmarshalJSON(data []byte) error {
	type plain ConversationQueueUpdate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "queue", "runs"}, false); err != nil {
		return err
	}
	*value = ConversationQueueUpdate(decoded)
	return nil
}
func (value ConversationQueueUpdate) MarshalJSON() ([]byte, error) {
	type plain ConversationQueueUpdate
	encoded := plain(value)
	encoded.Runs = normalizeCollections(encoded.Runs)
	return json.Marshal(encoded)
}

type ConversationDecision struct {
	Kind string
	Data any
}

var conversationDecisionVariants = variantFactories{"approve": nil,
	"deny":             nil,
	"approve_session":  nil,
	"approve_prefix":   func() any { return new(string) },
	"deny_with_reason": func() any { return new(string) }}

func (value ConversationDecision) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, conversationDecisionVariants)
}
func (value *ConversationDecision) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", conversationDecisionVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationApprovalChoice struct {
	Kind string
	Data any
}

var conversationApprovalChoiceVariants = variantFactories{"once": nil,
	"session": nil,
	"prefix":  func() any { return new(string) }}

func (value ConversationApprovalChoice) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, conversationApprovalChoiceVariants)
}
func (value *ConversationApprovalChoice) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", conversationApprovalChoiceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationApprovalState string

const (
	ConversationApprovalStatePending     ConversationApprovalState = "pending"
	ConversationApprovalStateApproved    ConversationApprovalState = "approved"
	ConversationApprovalStateDenied      ConversationApprovalState = "denied"
	ConversationApprovalStateCancelled   ConversationApprovalState = "cancelled"
	ConversationApprovalStateInterrupted ConversationApprovalState = "interrupted"
)

func (value ConversationApprovalState) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationApprovalState) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationApprovalState(decoded) {
	case ConversationApprovalStatePending, ConversationApprovalStateApproved, ConversationApprovalStateDenied, ConversationApprovalStateCancelled, ConversationApprovalStateInterrupted:
		*value = ConversationApprovalState(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationApprovalState) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationApprovalState) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationApprovalSource string

const (
	ConversationApprovalSourceStorage     ConversationApprovalSource = "storage"
	ConversationApprovalSourceSandbox     ConversationApprovalSource = "sandbox"
	ConversationApprovalSourceUser        ConversationApprovalSource = "user"
	ConversationApprovalSourceProject     ConversationApprovalSource = "project"
	ConversationApprovalSourceFull        ConversationApprovalSource = "full"
	ConversationApprovalSourceSessionRule ConversationApprovalSource = "session_rule"
	ConversationApprovalSourcePrefixRule  ConversationApprovalSource = "prefix_rule"
	ConversationApprovalSourceCodeRule    ConversationApprovalSource = "code_rule"
)

func (value ConversationApprovalSource) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConversationApprovalSource) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ConversationApprovalSource(decoded) {
	case ConversationApprovalSourceStorage, ConversationApprovalSourceSandbox, ConversationApprovalSourceUser, ConversationApprovalSourceProject, ConversationApprovalSourceFull, ConversationApprovalSourceSessionRule, ConversationApprovalSourcePrefixRule, ConversationApprovalSourceCodeRule:
		*value = ConversationApprovalSource(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ConversationApprovalSource) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ConversationApprovalSource) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ConversationApproval struct {
	ID          ApprovalID                  `json:"id"`
	Session     SessionID                   `json:"session"`
	Turn        TurnID                      `json:"turn"`
	Entry       string                      `json:"entry"`
	Index       uint64                      `json:"index"`
	State       ConversationApprovalState   `json:"state"`
	Source      ConversationApprovalSource  `json:"source"`
	Choice      *ConversationApprovalChoice `json:"choice"`
	Reason      *string                     `json:"reason"`
	Prefix      *string                     `json:"prefix"`
	CanRemember bool                        `json:"can_remember"`
	Startup     *CodeStartup                `json:"startup"`
	Ssh         *SshTrust                   `json:"ssh,omitempty"`
	Access      *ProcessAccess              `json:"access"`
	Changes     *ChangePreview              `json:"changes,omitempty"`
}

func DefaultConversationApproval() ConversationApproval {
	return ConversationApproval{
		ID:          ApprovalID{},
		Session:     SessionID{},
		Turn:        TurnID{},
		Entry:       "",
		Index:       0,
		State:       ConversationApprovalState(""),
		Source:      ConversationApprovalSource(""),
		Choice:      nil,
		Reason:      nil,
		Prefix:      nil,
		CanRemember: false,
		Startup:     nil,
		Ssh:         nil,
		Access:      nil,
		Changes:     nil,
	}
}
func (value *ConversationApproval) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationApproval) DecodeJSON(decoder *Decoder) error {
	type plain ConversationApproval
	decoded := plain{}
	decoded.Ssh = nil
	decoded.Changes = nil
	if err := decoder.Object(&decoded, []string{"id", "session", "turn", "entry", "index", "state", "source", "can_remember"}, false); err != nil {
		return err
	}
	*value = ConversationApproval(decoded)
	return nil
}
func (value ConversationApproval) MarshalJSON() ([]byte, error) {
	type plain ConversationApproval
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationImage struct {
	Entry      string     `json:"entry"`
	Part       uint64     `json:"part"`
	Index      uint64     `json:"index"`
	Attachment Attachment `json:"attachment"`
}

func DefaultConversationImage() ConversationImage {
	return ConversationImage{
		Entry:      "",
		Part:       0,
		Index:      0,
		Attachment: DefaultAttachment(),
	}
}
func (value *ConversationImage) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationImage) DecodeJSON(decoder *Decoder) error {
	type plain ConversationImage
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"entry", "part", "index", "attachment"}, false); err != nil {
		return err
	}
	*value = ConversationImage(decoded)
	return nil
}
func (value ConversationImage) MarshalJSON() ([]byte, error) {
	type plain ConversationImage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPart struct {
	Kind string
	Data any
}
type ConversationPartToolCallPayload struct {
	ID           *string            `json:"id"`
	Name         string             `json:"name"`
	Summary      *ToolDetailSummary `json:"summary"`
	Presentation ToolPresentation   `json:"presentation"`
	Grouping     ToolGrouping       `json:"grouping"`
}

func DefaultConversationPartToolCallPayload() ConversationPartToolCallPayload {
	return ConversationPartToolCallPayload{
		ID:           nil,
		Name:         "",
		Summary:      nil,
		Presentation: ToolPresentationSummary,
		Grouping:     ToolGroupingSequence,
	}
}
func (value *ConversationPartToolCallPayload) UnmarshalJSON(data []byte) error {
	return Decode(data, value)
}
func (value *ConversationPartToolCallPayload) DecodeJSON(decoder *Decoder) error {
	type plain ConversationPartToolCallPayload
	decoded := plain{}
	decoded.Presentation = ToolPresentationSummary
	decoded.Grouping = ToolGroupingSequence
	if err := decoder.Object(&decoded, []string{"name", "summary"}, false); err != nil {
		return err
	}
	*value = ConversationPartToolCallPayload(decoded)
	return nil
}
func (value ConversationPartToolCallPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPartToolCallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationPartToolResultPayload struct {
	ID      *string             `json:"id"`
	Name    string              `json:"name"`
	Summary *ToolDetailSummary  `json:"summary"`
	Images  []ConversationImage `json:"images"`
}

func DefaultConversationPartToolResultPayload() ConversationPartToolResultPayload {
	return ConversationPartToolResultPayload{
		ID:      nil,
		Name:    "",
		Summary: nil,
		Images:  []ConversationImage{},
	}
}
func (value *ConversationPartToolResultPayload) UnmarshalJSON(data []byte) error {
	return Decode(data, value)
}
func (value *ConversationPartToolResultPayload) DecodeJSON(decoder *Decoder) error {
	type plain ConversationPartToolResultPayload
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"name", "summary", "images"}, false); err != nil {
		return err
	}
	*value = ConversationPartToolResultPayload(decoded)
	return nil
}
func (value ConversationPartToolResultPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationPartToolResultPayload
	encoded := plain(value)
	encoded.Images = normalizeCollections(encoded.Images)
	return json.Marshal(encoded)
}

var conversationPartVariants = variantFactories{"text": func() any { return new(string) },
	"reference":   func() any { return new(ConversationReference) },
	"attachment":  func() any { return new(Attachment) },
	"image":       func() any { return new(ConversationImage) },
	"thinking":    func() any { return new(string) },
	"compaction":  func() any { return new(string) },
	"tool_call":   func() any { return new(ConversationPartToolCallPayload) },
	"tool_result": func() any { return new(ConversationPartToolResultPayload) },
	"resource":    func() any { return new(json.RawMessage) }}

func (value ConversationPart) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationPartVariants)
}
func (value *ConversationPart) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationPart) DecodeJSON(decoder *Decoder) error {
	kind, decoded, err := decoder.Union("kind", "data", conversationPartVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationUsage struct {
	Input       uint64 `json:"input"`
	Output      uint64 `json:"output"`
	CachedInput uint64 `json:"cached_input"`
	Reasoning   uint64 `json:"reasoning"`
}

func DefaultConversationUsage() ConversationUsage {
	return ConversationUsage{
		Input:       0,
		Output:      0,
		CachedInput: 0,
		Reasoning:   0,
	}
}
func (value *ConversationUsage) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationUsage) DecodeJSON(decoder *Decoder) error {
	type plain ConversationUsage
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"input", "output", "cached_input", "reasoning"}, false); err != nil {
		return err
	}
	*value = ConversationUsage(decoded)
	return nil
}
func (value ConversationUsage) MarshalJSON() ([]byte, error) {
	type plain ConversationUsage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationStatistics struct {
	Observations       ConversationObservationSummary `json:"observations"`
	Turns              uint64                         `json:"turns"`
	Responses          uint64                         `json:"responses"`
	Usage              *ConversationUsage             `json:"usage"`
	ContextTokens      *uint64                        `json:"context_tokens"`
	ContextEstimate    *uint64                        `json:"context_estimate"`
	ContextLimit       *uint32                        `json:"context_limit"`
	ContextModel       *string                        `json:"context_model"`
	ContextTimestampMs *int64                         `json:"context_timestamp_ms"`
	Budget             *ConversationCompactionBudget  `json:"budget"`
	LastCompaction     *ConversationCompactionRecord  `json:"last_compaction"`
	FirstTokenUs       *uint64                        `json:"first_token_us"`
	Cost               *UsageCost                     `json:"cost"`
	Generation         *UsageGeneration               `json:"generation"`
	Delegated          UsageMetrics                   `json:"delegated"`
}

func DefaultConversationStatistics() ConversationStatistics {
	return ConversationStatistics{
		Observations:       DefaultConversationObservationSummary(),
		Turns:              0,
		Responses:          0,
		Usage:              nil,
		ContextTokens:      nil,
		ContextEstimate:    nil,
		ContextLimit:       nil,
		ContextModel:       nil,
		ContextTimestampMs: nil,
		Budget:             nil,
		LastCompaction:     nil,
		FirstTokenUs:       nil,
		Cost:               nil,
		Generation:         nil,
		Delegated:          DefaultUsageMetrics(),
	}
}
func (value *ConversationStatistics) UnmarshalJSON(data []byte) error {
	type plain ConversationStatistics
	decoded := plain{}
	decoded.Observations = DefaultConversationObservationSummary()
	decoded.ContextEstimate = nil
	decoded.ContextLimit = nil
	decoded.ContextModel = nil
	decoded.ContextTimestampMs = nil
	decoded.Budget = nil
	decoded.LastCompaction = nil
	decoded.FirstTokenUs = nil
	decoded.Delegated = DefaultUsageMetrics()
	if err := DecodeObject(data, &decoded, []string{"turns", "responses"}, false); err != nil {
		return err
	}
	*value = ConversationStatistics(decoded)
	return nil
}
func (value ConversationStatistics) MarshalJSON() ([]byte, error) {
	type plain ConversationStatistics
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationCitation struct {
	Uri   string  `json:"uri"`
	Title *string `json:"title"`
	Start *uint32 `json:"start"`
	End   *uint32 `json:"end"`
}

func DefaultConversationCitation() ConversationCitation {
	return ConversationCitation{
		Uri:   "",
		Title: nil,
		Start: nil,
		End:   nil,
	}
}
func (value *ConversationCitation) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationCitation) DecodeJSON(decoder *Decoder) error {
	type plain ConversationCitation
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"uri"}, false); err != nil {
		return err
	}
	*value = ConversationCitation(decoded)
	return nil
}
func (value ConversationCitation) MarshalJSON() ([]byte, error) {
	type plain ConversationCitation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationEntry struct {
	Sequence          uint64                 `json:"sequence"`
	ID                string                 `json:"id"`
	Turn              TurnID                 `json:"turn"`
	Author            string                 `json:"author"`
	Branch            string                 `json:"branch"`
	TimestampMs       int64                  `json:"timestamp_ms"`
	Parts             []ConversationPart     `json:"parts"`
	Citations         []ConversationCitation `json:"citations"`
	SearchSuggestions *string                `json:"search_suggestions"`
	Usage             *ConversationUsage     `json:"usage"`
}

func DefaultConversationEntry() ConversationEntry {
	return ConversationEntry{
		Sequence:          0,
		ID:                "",
		Turn:              TurnID{},
		Author:            "",
		Branch:            "",
		TimestampMs:       0,
		Parts:             []ConversationPart{},
		Citations:         []ConversationCitation{},
		SearchSuggestions: nil,
		Usage:             nil,
	}
}
func (value *ConversationEntry) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationEntry) DecodeJSON(decoder *Decoder) error {
	type plain ConversationEntry
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"sequence", "id", "turn", "author", "branch", "timestamp_ms", "parts", "citations"}, false); err != nil {
		return err
	}
	*value = ConversationEntry(decoded)
	return nil
}
func (value ConversationEntry) MarshalJSON() ([]byte, error) {
	type plain ConversationEntry
	encoded := plain(value)
	encoded.Parts = normalizeCollections(encoded.Parts)
	encoded.Citations = normalizeCollections(encoded.Citations)
	return json.Marshal(encoded)
}

type ConversationPage struct {
	Session    SessionID                    `json:"session"`
	Revision   uint64                       `json:"revision"`
	Entries    []ConversationEntry          `json:"entries"`
	Runs       []ConversationRun            `json:"runs"`
	Queue      ConversationQueue            `json:"queue"`
	Approvals  []ConversationApproval       `json:"approvals"`
	Questions  []ConversationQuestion       `json:"questions"`
	Children   []ConversationChild          `json:"children"`
	Background []ConversationBackgroundTask `json:"background"`
	NextBefore *TurnID                      `json:"next_before"`
}

func DefaultConversationPage() ConversationPage {
	return ConversationPage{
		Session:    SessionID{},
		Revision:   0,
		Entries:    []ConversationEntry{},
		Runs:       []ConversationRun{},
		Queue:      DefaultConversationQueue(),
		Approvals:  []ConversationApproval{},
		Questions:  []ConversationQuestion{},
		Children:   []ConversationChild{},
		Background: []ConversationBackgroundTask{},
		NextBefore: nil,
	}
}
func (value *ConversationPage) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationPage) DecodeJSON(decoder *Decoder) error {
	type plain ConversationPage
	decoded := plain{}
	decoded.Background = []ConversationBackgroundTask{}
	if err := decoder.Object(&decoded, []string{"session", "revision", "entries", "runs", "queue", "approvals", "questions", "children"}, false); err != nil {
		return err
	}
	*value = ConversationPage(decoded)
	return nil
}
func (value ConversationPage) MarshalJSON() ([]byte, error) {
	type plain ConversationPage
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	encoded.Runs = normalizeCollections(encoded.Runs)
	encoded.Approvals = normalizeCollections(encoded.Approvals)
	encoded.Questions = normalizeCollections(encoded.Questions)
	encoded.Children = normalizeCollections(encoded.Children)
	encoded.Background = normalizeCollections(encoded.Background)
	return json.Marshal(encoded)
}

type ConversationHistory struct {
	Sequence uint64           `json:"sequence"`
	Page     ConversationPage `json:"page"`
	Missing  []TurnID         `json:"missing"`
}

func DefaultConversationHistory() ConversationHistory {
	return ConversationHistory{
		Sequence: 0,
		Page:     DefaultConversationPage(),
		Missing:  []TurnID{},
	}
}
func (value *ConversationHistory) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConversationHistory) DecodeJSON(decoder *Decoder) error {
	type plain ConversationHistory
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"sequence", "page", "missing"}, false); err != nil {
		return err
	}
	*value = ConversationHistory(decoded)
	return nil
}
func (value ConversationHistory) MarshalJSON() ([]byte, error) {
	type plain ConversationHistory
	encoded := plain(value)
	encoded.Missing = normalizeCollections(encoded.Missing)
	return json.Marshal(encoded)
}

type ConversationTurnHistory struct {
	Sequence   uint64                 `json:"sequence"`
	Revision   uint64                 `json:"revision"`
	Run        ConversationRun        `json:"run"`
	Entries    []ConversationEntry    `json:"entries"`
	Approvals  []ConversationApproval `json:"approvals"`
	Questions  []ConversationQuestion `json:"questions"`
	Children   []ConversationChild    `json:"children"`
	NextBefore *uint64                `json:"next_before"`
}

func DefaultConversationTurnHistory() ConversationTurnHistory {
	return ConversationTurnHistory{
		Sequence:   0,
		Revision:   0,
		Run:        DefaultConversationRun(),
		Entries:    []ConversationEntry{},
		Approvals:  []ConversationApproval{},
		Questions:  []ConversationQuestion{},
		Children:   []ConversationChild{},
		NextBefore: nil,
	}
}
func (value *ConversationTurnHistory) UnmarshalJSON(data []byte) error {
	type plain ConversationTurnHistory
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sequence", "revision", "run", "entries", "approvals", "questions", "children"}, false); err != nil {
		return err
	}
	*value = ConversationTurnHistory(decoded)
	return nil
}
func (value ConversationTurnHistory) MarshalJSON() ([]byte, error) {
	type plain ConversationTurnHistory
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	encoded.Approvals = normalizeCollections(encoded.Approvals)
	encoded.Questions = normalizeCollections(encoded.Questions)
	encoded.Children = normalizeCollections(encoded.Children)
	return json.Marshal(encoded)
}

type ConversationDraft struct {
	ID     string             `json:"id"`
	Turn   TurnID             `json:"turn"`
	Author string             `json:"author"`
	Branch string             `json:"branch"`
	Parts  []ConversationPart `json:"parts"`
}

func DefaultConversationDraft() ConversationDraft {
	return ConversationDraft{
		ID:     "",
		Turn:   TurnID{},
		Author: "",
		Branch: "",
		Parts:  []ConversationPart{},
	}
}
func (value *ConversationDraft) UnmarshalJSON(data []byte) error {
	type plain ConversationDraft
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "turn", "author", "branch", "parts"}, false); err != nil {
		return err
	}
	*value = ConversationDraft(decoded)
	return nil
}
func (value ConversationDraft) MarshalJSON() ([]byte, error) {
	type plain ConversationDraft
	encoded := plain(value)
	encoded.Parts = normalizeCollections(encoded.Parts)
	return json.Marshal(encoded)
}

type ConversationSnapshot struct {
	Node       NodeID                  `json:"node"`
	Sequence   uint64                  `json:"sequence"`
	Page       ConversationPage        `json:"page"`
	Missing    []TurnID                `json:"missing"`
	Drafts     []ConversationDraft     `json:"drafts"`
	Statistics *ConversationStatistics `json:"statistics"`
	Artifacts  ArtifactPage            `json:"artifacts"`
}

func DefaultConversationSnapshot() ConversationSnapshot {
	return ConversationSnapshot{
		Node:       NodeID{},
		Sequence:   0,
		Page:       DefaultConversationPage(),
		Missing:    []TurnID{},
		Drafts:     []ConversationDraft{},
		Statistics: nil,
		Artifacts:  DefaultArtifactPage(),
	}
}
func (value *ConversationSnapshot) UnmarshalJSON(data []byte) error {
	type plain ConversationSnapshot
	decoded := plain{}
	decoded.Artifacts = DefaultArtifactPage()
	if err := DecodeObject(data, &decoded, []string{"node", "sequence", "page", "missing", "drafts"}, false); err != nil {
		return err
	}
	*value = ConversationSnapshot(decoded)
	return nil
}
func (value ConversationSnapshot) MarshalJSON() ([]byte, error) {
	type plain ConversationSnapshot
	encoded := plain(value)
	encoded.Missing = normalizeCollections(encoded.Missing)
	encoded.Drafts = normalizeCollections(encoded.Drafts)
	return json.Marshal(encoded)
}

type ConversationChange struct {
	Kind string
	Data any
}
type ConversationChangeHiddenPayload struct {
	Turn TurnID `json:"turn"`
	ID   string `json:"id"`
}

func DefaultConversationChangeHiddenPayload() ConversationChangeHiddenPayload {
	return ConversationChangeHiddenPayload{
		Turn: TurnID{},
		ID:   "",
	}
}
func (value *ConversationChangeHiddenPayload) UnmarshalJSON(data []byte) error {
	type plain ConversationChangeHiddenPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "id"}, false); err != nil {
		return err
	}
	*value = ConversationChangeHiddenPayload(decoded)
	return nil
}
func (value ConversationChangeHiddenPayload) MarshalJSON() ([]byte, error) {
	type plain ConversationChangeHiddenPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var conversationChangeVariants = variantFactories{"background": func() any { return new(ConversationBackgroundTask) },
	"artifact":          func() any { return new(ArtifactSummary) },
	"entry":             func() any { return new(ConversationEntry) },
	"revealed":          func() any { return new(ConversationEntry) },
	"hidden":            func() any { return new(ConversationChangeHiddenPayload) },
	"statistics":        func() any { return new(ConversationStatistics) },
	"statistics_failed": func() any { return new(Fault) },
	"run":               func() any { return new(ConversationRun) },
	"queue":             func() any { return new(ConversationQueue) },
	"approval":          func() any { return new(ConversationApproval) },
	"question":          func() any { return new(ConversationQuestion) },
	"child":             func() any { return new(ConversationChild) },
	"delta":             func() any { return new(ConversationDraft) }}

func (value ConversationChange) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, conversationChangeVariants)
}
func (value *ConversationChange) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", conversationChangeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConversationFrame struct {
	Node     NodeID             `json:"node"`
	Session  SessionID          `json:"session"`
	Sequence uint64             `json:"sequence"`
	Change   ConversationChange `json:"change"`
}

func DefaultConversationFrame() ConversationFrame {
	return ConversationFrame{
		Node:     NodeID{},
		Session:  SessionID{},
		Sequence: 0,
		Change:   ConversationChange{},
	}
}
func (value *ConversationFrame) UnmarshalJSON(data []byte) error {
	type plain ConversationFrame
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "session", "sequence", "change"}, false); err != nil {
		return err
	}
	*value = ConversationFrame(decoded)
	return nil
}
func (value ConversationFrame) MarshalJSON() ([]byte, error) {
	type plain ConversationFrame
	encoded := plain(value)
	return json.Marshal(encoded)
}
