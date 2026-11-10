// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ErrorCode string

const (
	ErrorCodeInvalidRequest   ErrorCode = "invalid_request"
	ErrorCodeWrongTarget      ErrorCode = "wrong_target"
	ErrorCodeNotFound         ErrorCode = "not_found"
	ErrorCodeNotConfigured    ErrorCode = "not_configured"
	ErrorCodeConflict         ErrorCode = "conflict"
	ErrorCodeRevisionConflict ErrorCode = "revision_conflict"
	ErrorCodePermissionDenied ErrorCode = "permission_denied"
	ErrorCodeExpired          ErrorCode = "expired"
	ErrorCodeBusy             ErrorCode = "busy"
	ErrorCodeUnavailable      ErrorCode = "unavailable"
	ErrorCodeCancelled        ErrorCode = "cancelled"
	ErrorCodeOutcomeUnknown   ErrorCode = "outcome_unknown"
	ErrorCodeInternal         ErrorCode = "internal"
)

func (value ErrorCode) MarshalJSON() ([]byte, error) {
	switch value {
	case ErrorCodeInvalidRequest, ErrorCodeWrongTarget, ErrorCodeNotFound, ErrorCodeNotConfigured, ErrorCodeConflict, ErrorCodeRevisionConflict, ErrorCodePermissionDenied, ErrorCodeExpired, ErrorCodeBusy, ErrorCodeUnavailable, ErrorCodeCancelled, ErrorCodeOutcomeUnknown, ErrorCodeInternal:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ErrorCode %q", value)
	}
}
func (value *ErrorCode) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ErrorCode(decoded) {
	case ErrorCodeInvalidRequest, ErrorCodeWrongTarget, ErrorCodeNotFound, ErrorCodeNotConfigured, ErrorCodeConflict, ErrorCodeRevisionConflict, ErrorCodePermissionDenied, ErrorCodeExpired, ErrorCodeBusy, ErrorCodeUnavailable, ErrorCodeCancelled, ErrorCodeOutcomeUnknown, ErrorCodeInternal:
		*value = ErrorCode(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ErrorCode %q", decoded)
	}
}
func (value ErrorCode) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ErrorCode) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type Fault struct {
	Code    ErrorCode     `json:"code"`
	Message string        `json:"message"`
	Details *FaultDetails `json:"details,omitempty"`
}

func DefaultFault() Fault {
	return Fault{
		Code:    ErrorCode(""),
		Message: "",
		Details: nil,
	}
}
func (value *Fault) UnmarshalJSON(data []byte) error {
	type plain Fault
	decoded := plain{}
	decoded.Details = nil
	if err := DecodeObject(data, &decoded, []string{"code", "message"}, false); err != nil {
		return err
	}
	*value = Fault(decoded)
	return nil
}
func (value Fault) MarshalJSON() ([]byte, error) {
	type plain Fault
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FaultDetails struct {
	Kind string
	Data any
}
type FaultDetailsPluginRequiresNewerPayload struct {
	Required string `json:"required"`
}

func DefaultFaultDetailsPluginRequiresNewerPayload() FaultDetailsPluginRequiresNewerPayload {
	return FaultDetailsPluginRequiresNewerPayload{
		Required: "",
	}
}
func (value *FaultDetailsPluginRequiresNewerPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsPluginRequiresNewerPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"required"}, false); err != nil {
		return err
	}
	*value = FaultDetailsPluginRequiresNewerPayload(decoded)
	return nil
}
func (value FaultDetailsPluginRequiresNewerPayload) MarshalJSON() ([]byte, error) {
	type plain FaultDetailsPluginRequiresNewerPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FaultDetailsModelRecoveryPayload struct {
	Reason ConversationRecoveryStop `json:"reason"`
}

func DefaultFaultDetailsModelRecoveryPayload() FaultDetailsModelRecoveryPayload {
	return FaultDetailsModelRecoveryPayload{
		Reason: ConversationRecoveryStop(""),
	}
}
func (value *FaultDetailsModelRecoveryPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsModelRecoveryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"reason"}, false); err != nil {
		return err
	}
	*value = FaultDetailsModelRecoveryPayload(decoded)
	return nil
}
func (value FaultDetailsModelRecoveryPayload) MarshalJSON() ([]byte, error) {
	type plain FaultDetailsModelRecoveryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FaultDetailsModelRequestPayload struct {
	Reason ModelFailure `json:"reason"`
}

func DefaultFaultDetailsModelRequestPayload() FaultDetailsModelRequestPayload {
	return FaultDetailsModelRequestPayload{
		Reason: ModelFailure(""),
	}
}
func (value *FaultDetailsModelRequestPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsModelRequestPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"reason"}, false); err != nil {
		return err
	}
	*value = FaultDetailsModelRequestPayload(decoded)
	return nil
}
func (value FaultDetailsModelRequestPayload) MarshalJSON() ([]byte, error) {
	type plain FaultDetailsModelRequestPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FaultDetailsVersionMismatchPayload struct {
	Controller         *string `json:"controller"`
	Host               *string `json:"host"`
	ControllerProtocol *uint16 `json:"controller_protocol"`
	HostProtocol       *uint16 `json:"host_protocol"`
	UpdateHost         bool    `json:"update_host"`
	Unverified         bool    `json:"unverified"`
}

func DefaultFaultDetailsVersionMismatchPayload() FaultDetailsVersionMismatchPayload {
	return FaultDetailsVersionMismatchPayload{
		Controller:         nil,
		Host:               nil,
		ControllerProtocol: nil,
		HostProtocol:       nil,
		UpdateHost:         false,
		Unverified:         false,
	}
}
func (value *FaultDetailsVersionMismatchPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsVersionMismatchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"update_host", "unverified"}, false); err != nil {
		return err
	}
	*value = FaultDetailsVersionMismatchPayload(decoded)
	return nil
}
func (value FaultDetailsVersionMismatchPayload) MarshalJSON() ([]byte, error) {
	type plain FaultDetailsVersionMismatchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FaultDetailsProjectRemovalPayload struct {
	TasksActive          bool `json:"tasks_active"`
	TerminalsMayBeClosed bool `json:"terminals_may_be_closed"`
}

func DefaultFaultDetailsProjectRemovalPayload() FaultDetailsProjectRemovalPayload {
	return FaultDetailsProjectRemovalPayload{
		TasksActive:          false,
		TerminalsMayBeClosed: false,
	}
}
func (value *FaultDetailsProjectRemovalPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsProjectRemovalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tasks_active", "terminals_may_be_closed"}, false); err != nil {
		return err
	}
	*value = FaultDetailsProjectRemovalPayload(decoded)
	return nil
}
func (value FaultDetailsProjectRemovalPayload) MarshalJSON() ([]byte, error) {
	type plain FaultDetailsProjectRemovalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var faultDetailsVariants = variantFactories{"plugin_requires_newer": func() any { return new(FaultDetailsPluginRequiresNewerPayload) },
	"storage_conflict":        nil,
	"plugin_value_conflict":   nil,
	"worktree_path_not_found": nil,
	"network":                 nil,
	"model_recovery":          func() any { return new(FaultDetailsModelRecoveryPayload) },
	"model_request":           func() any { return new(FaultDetailsModelRequestPayload) },
	"search_unsupported":      nil,
	"system_diagnostic":       func() any { return new(NotificationSystemSystemDiagnostic) },
	"version_mismatch":        func() any { return new(FaultDetailsVersionMismatchPayload) },
	"project_removal":         func() any { return new(FaultDetailsProjectRemovalPayload) }}

func (value FaultDetails) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, faultDetailsVariants)
}
func (value *FaultDetails) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", faultDetailsVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ModelFailure string

const (
	ModelFailureRequest                ModelFailure = "request"
	ModelFailureUnauthorized           ModelFailure = "unauthorized"
	ModelFailureRateLimited            ModelFailure = "rate_limited"
	ModelFailureTimeout                ModelFailure = "timeout"
	ModelFailureInvalidResponse        ModelFailure = "invalid_response"
	ModelFailureSharedUsageLimit       ModelFailure = "shared_usage_limit"
	ModelFailureSharedUsageUnavailable ModelFailure = "shared_usage_unavailable"
)

func (value ModelFailure) MarshalJSON() ([]byte, error) {
	switch value {
	case ModelFailureRequest, ModelFailureUnauthorized, ModelFailureRateLimited, ModelFailureTimeout, ModelFailureInvalidResponse, ModelFailureSharedUsageLimit, ModelFailureSharedUsageUnavailable:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ModelFailure %q", value)
	}
}
func (value *ModelFailure) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ModelFailure(decoded) {
	case ModelFailureRequest, ModelFailureUnauthorized, ModelFailureRateLimited, ModelFailureTimeout, ModelFailureInvalidResponse, ModelFailureSharedUsageLimit, ModelFailureSharedUsageUnavailable:
		*value = ModelFailure(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ModelFailure %q", decoded)
	}
}
func (value ModelFailure) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ModelFailure) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type Project struct {
	ID         ProjectID          `json:"id"`
	Name       string             `json:"name"`
	Path       string             `json:"path"`
	Appearance ProjectsAppearance `json:"appearance"`
}

func DefaultProject() Project {
	return Project{
		ID:         ProjectID{},
		Name:       "",
		Path:       "",
		Appearance: DefaultProjectsAppearance(),
	}
}
func (value *Project) UnmarshalJSON(data []byte) error {
	type plain Project
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "name", "path", "appearance"}, false); err != nil {
		return err
	}
	*value = Project(decoded)
	return nil
}
func (value Project) MarshalJSON() ([]byte, error) {
	type plain Project
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Worktree struct {
	ID      WorktreeID `json:"id"`
	Project *ProjectID `json:"project"`
	Path    string     `json:"path"`
	Main    bool       `json:"main"`
}

func DefaultWorktree() Worktree {
	return Worktree{
		ID:      WorktreeID{},
		Project: nil,
		Path:    "",
		Main:    false,
	}
}
func (value *Worktree) UnmarshalJSON(data []byte) error {
	type plain Worktree
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "path", "main"}, false); err != nil {
		return err
	}
	*value = Worktree(decoded)
	return nil
}
func (value Worktree) MarshalJSON() ([]byte, error) {
	type plain Worktree
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Command struct {
	Kind string
	Data any
}
type CommandReadArtifactPayload struct {
	ID       ArtifactID `json:"id"`
	Revision *uint32    `json:"revision"`
	Session  *SessionID `json:"session"`
}

func DefaultCommandReadArtifactPayload() CommandReadArtifactPayload {
	return CommandReadArtifactPayload{
		ID:       ArtifactID{},
		Revision: nil,
		Session:  nil,
	}
}
func (value *CommandReadArtifactPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadArtifactPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = CommandReadArtifactPayload(decoded)
	return nil
}
func (value CommandReadArtifactPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadArtifactPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListArtifactsPayload struct {
	Session SessionID   `json:"session"`
	After   *ArtifactID `json:"after"`
	Limit   uint16      `json:"limit"`
}

func DefaultCommandListArtifactsPayload() CommandListArtifactsPayload {
	return CommandListArtifactsPayload{
		Session: SessionID{},
		After:   nil,
		Limit:   0,
	}
}
func (value *CommandListArtifactsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListArtifactsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "limit"}, false); err != nil {
		return err
	}
	*value = CommandListArtifactsPayload(decoded)
	return nil
}
func (value CommandListArtifactsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListArtifactsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListArtifactRevisionsPayload struct {
	ID      ArtifactID `json:"id"`
	Before  *uint32    `json:"before"`
	Limit   uint16     `json:"limit"`
	Session *SessionID `json:"session"`
}

func DefaultCommandListArtifactRevisionsPayload() CommandListArtifactRevisionsPayload {
	return CommandListArtifactRevisionsPayload{
		ID:      ArtifactID{},
		Before:  nil,
		Limit:   0,
		Session: nil,
	}
}
func (value *CommandListArtifactRevisionsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListArtifactRevisionsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "limit"}, false); err != nil {
		return err
	}
	*value = CommandListArtifactRevisionsPayload(decoded)
	return nil
}
func (value CommandListArtifactRevisionsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListArtifactRevisionsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPluginTransactionPayload struct {
	Operations []PluginTransactionOperation `json:"operations"`
}

func DefaultCommandPluginTransactionPayload() CommandPluginTransactionPayload {
	return CommandPluginTransactionPayload{
		Operations: []PluginTransactionOperation{},
	}
}
func (value *CommandPluginTransactionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandPluginTransactionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"operations"}, false); err != nil {
		return err
	}
	*value = CommandPluginTransactionPayload(decoded)
	return nil
}
func (value CommandPluginTransactionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandPluginTransactionPayload
	encoded := plain(value)
	encoded.Operations = normalizeCollections(encoded.Operations)
	return json.Marshal(encoded)
}

type CommandCallPluginPayload struct {
	Handler string          `json:"handler"`
	Input   json.RawMessage `json:"input"`
}

func DefaultCommandCallPluginPayload() CommandCallPluginPayload {
	return CommandCallPluginPayload{
		Handler: "",
		Input:   json.RawMessage("null"),
	}
}
func (value *CommandCallPluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCallPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"handler", "input"}, false); err != nil {
		return err
	}
	*value = CommandCallPluginPayload(decoded)
	return nil
}
func (value CommandCallPluginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCallPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginCallbackPayload struct {
	Handler string          `json:"handler"`
	Input   json.RawMessage `json:"input"`
}

func DefaultCommandReadPluginCallbackPayload() CommandReadPluginCallbackPayload {
	return CommandReadPluginCallbackPayload{
		Handler: "",
		Input:   json.RawMessage("null"),
	}
}
func (value *CommandReadPluginCallbackPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginCallbackPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"handler", "input"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginCallbackPayload(decoded)
	return nil
}
func (value CommandReadPluginCallbackPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginCallbackPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelPluginCallPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelPluginCallPayload() CommandCancelPluginCallPayload {
	return CommandCancelPluginCallPayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelPluginCallPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelPluginCallPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelPluginCallPayload(decoded)
	return nil
}
func (value CommandCancelPluginCallPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelPluginCallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPublishNotificationPayload struct {
	Package PluginReference   `json:"package"`
	Content NotificationDraft `json:"content"`
}

func DefaultCommandPublishNotificationPayload() CommandPublishNotificationPayload {
	return CommandPublishNotificationPayload{
		Package: DefaultPluginReference(),
		Content: DefaultNotificationDraft(),
	}
}
func (value *CommandPublishNotificationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandPublishNotificationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "content"}, false); err != nil {
		return err
	}
	*value = CommandPublishNotificationPayload(decoded)
	return nil
}
func (value CommandPublishNotificationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandPublishNotificationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMarkNotificationReadPayload struct {
	ID NotificationID `json:"id"`
}

func DefaultCommandMarkNotificationReadPayload() CommandMarkNotificationReadPayload {
	return CommandMarkNotificationReadPayload{
		ID: NotificationID{},
	}
}
func (value *CommandMarkNotificationReadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandMarkNotificationReadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = CommandMarkNotificationReadPayload(decoded)
	return nil
}
func (value CommandMarkNotificationReadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandMarkNotificationReadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMarkNotificationsReadPayload struct {
	IDs []NotificationID `json:"ids"`
}

func DefaultCommandMarkNotificationsReadPayload() CommandMarkNotificationsReadPayload {
	return CommandMarkNotificationsReadPayload{
		IDs: []NotificationID{},
	}
}
func (value *CommandMarkNotificationsReadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandMarkNotificationsReadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids"}, false); err != nil {
		return err
	}
	*value = CommandMarkNotificationsReadPayload(decoded)
	return nil
}
func (value CommandMarkNotificationsReadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandMarkNotificationsReadPayload
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

type CommandDismissReadNotificationsPayload struct {
	IDs []NotificationID `json:"ids"`
}

func DefaultCommandDismissReadNotificationsPayload() CommandDismissReadNotificationsPayload {
	return CommandDismissReadNotificationsPayload{
		IDs: []NotificationID{},
	}
}
func (value *CommandDismissReadNotificationsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDismissReadNotificationsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids"}, false); err != nil {
		return err
	}
	*value = CommandDismissReadNotificationsPayload(decoded)
	return nil
}
func (value CommandDismissReadNotificationsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDismissReadNotificationsPayload
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

type CommandDismissNotificationsPayload struct {
	IDs []NotificationID `json:"ids"`
}

func DefaultCommandDismissNotificationsPayload() CommandDismissNotificationsPayload {
	return CommandDismissNotificationsPayload{
		IDs: []NotificationID{},
	}
}
func (value *CommandDismissNotificationsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDismissNotificationsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids"}, false); err != nil {
		return err
	}
	*value = CommandDismissNotificationsPayload(decoded)
	return nil
}
func (value CommandDismissNotificationsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDismissNotificationsPayload
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

type CommandDispatchPayload struct {
	Package PluginReference `json:"package"`
	Action  DispatchCommand `json:"action"`
}

func DefaultCommandDispatchPayload() CommandDispatchPayload {
	return CommandDispatchPayload{
		Package: DefaultPluginReference(),
		Action:  DispatchCommand{},
	}
}
func (value *CommandDispatchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDispatchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "action"}, false); err != nil {
		return err
	}
	*value = CommandDispatchPayload(decoded)
	return nil
}
func (value CommandDispatchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDispatchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStopDispatchTurnPayload struct {
	Package PluginReference `json:"package"`
	Job     JobID           `json:"job"`
}

func DefaultCommandStopDispatchTurnPayload() CommandStopDispatchTurnPayload {
	return CommandStopDispatchTurnPayload{
		Package: DefaultPluginReference(),
		Job:     JobID{},
	}
}
func (value *CommandStopDispatchTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStopDispatchTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "job"}, false); err != nil {
		return err
	}
	*value = CommandStopDispatchTurnPayload(decoded)
	return nil
}
func (value CommandStopDispatchTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStopDispatchTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUseBrowserPayload struct {
	Session  SessionID     `json:"session"`
	Worktree WorktreeID    `json:"worktree"`
	Action   BrowserAction `json:"action"`
}

func DefaultCommandUseBrowserPayload() CommandUseBrowserPayload {
	return CommandUseBrowserPayload{
		Session:  SessionID{},
		Worktree: WorktreeID{},
		Action:   BrowserAction{},
	}
}
func (value *CommandUseBrowserPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUseBrowserPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "worktree", "action"}, false); err != nil {
		return err
	}
	*value = CommandUseBrowserPayload(decoded)
	return nil
}
func (value CommandUseBrowserPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUseBrowserPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUseExternalBrowserPayload struct {
	Session   SessionID             `json:"session"`
	Worktree  WorktreeID            `json:"worktree"`
	Action    ExternalBrowserAction `json:"action"`
	Arguments json.RawMessage       `json:"arguments"`
}

func DefaultCommandUseExternalBrowserPayload() CommandUseExternalBrowserPayload {
	return CommandUseExternalBrowserPayload{
		Session:   SessionID{},
		Worktree:  WorktreeID{},
		Action:    ExternalBrowserAction(""),
		Arguments: json.RawMessage("null"),
	}
}
func (value *CommandUseExternalBrowserPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUseExternalBrowserPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "worktree", "action", "arguments"}, false); err != nil {
		return err
	}
	*value = CommandUseExternalBrowserPayload(decoded)
	return nil
}
func (value CommandUseExternalBrowserPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUseExternalBrowserPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCompleteBrowserPayload struct {
	ID     RequestID     `json:"id"`
	Result BrowserResult `json:"result"`
}

func DefaultCommandCompleteBrowserPayload() CommandCompleteBrowserPayload {
	return CommandCompleteBrowserPayload{
		ID:     RequestID{},
		Result: Result[json.RawMessage, Fault]{},
	}
}
func (value *CommandCompleteBrowserPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCompleteBrowserPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "result"}, false); err != nil {
		return err
	}
	*value = CommandCompleteBrowserPayload(decoded)
	return nil
}
func (value CommandCompleteBrowserPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCompleteBrowserPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginValuePayload struct {
	Key string `json:"key"`
}

func DefaultCommandReadPluginValuePayload() CommandReadPluginValuePayload {
	return CommandReadPluginValuePayload{
		Key: "",
	}
}
func (value *CommandReadPluginValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginValuePayload(decoded)
	return nil
}
func (value CommandReadPluginValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginConversationValuePayload struct {
	Key string `json:"key"`
}

func DefaultCommandReadPluginConversationValuePayload() CommandReadPluginConversationValuePayload {
	return CommandReadPluginConversationValuePayload{
		Key: "",
	}
}
func (value *CommandReadPluginConversationValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginConversationValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginConversationValuePayload(decoded)
	return nil
}
func (value CommandReadPluginConversationValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginConversationValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListPluginKeysPayload struct {
	Prefix string  `json:"prefix"`
	After  *string `json:"after"`
	Limit  uint16  `json:"limit"`
}

func DefaultCommandListPluginKeysPayload() CommandListPluginKeysPayload {
	return CommandListPluginKeysPayload{
		Prefix: "",
		After:  nil,
		Limit:  0,
	}
}
func (value *CommandListPluginKeysPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListPluginKeysPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"prefix", "limit"}, false); err != nil {
		return err
	}
	*value = CommandListPluginKeysPayload(decoded)
	return nil
}
func (value CommandListPluginKeysPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListPluginKeysPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandWritePluginValuePayload struct {
	Key              string          `json:"key"`
	Value            json.RawMessage `json:"value"`
	ExpectedRevision uint64          `json:"expected_revision"`
}

func DefaultCommandWritePluginValuePayload() CommandWritePluginValuePayload {
	return CommandWritePluginValuePayload{
		Key:              "",
		Value:            json.RawMessage("null"),
		ExpectedRevision: 0,
	}
}
func (value *CommandWritePluginValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandWritePluginValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "value", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandWritePluginValuePayload(decoded)
	return nil
}
func (value CommandWritePluginValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandWritePluginValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandWriteIndexedPluginValuePayload struct {
	Key              string             `json:"key"`
	Value            json.RawMessage    `json:"value"`
	Index            PluginStorageIndex `json:"index"`
	ExpectedRevision uint64             `json:"expected_revision"`
}

func DefaultCommandWriteIndexedPluginValuePayload() CommandWriteIndexedPluginValuePayload {
	return CommandWriteIndexedPluginValuePayload{
		Key:              "",
		Value:            json.RawMessage("null"),
		Index:            DefaultPluginStorageIndex(),
		ExpectedRevision: 0,
	}
}
func (value *CommandWriteIndexedPluginValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandWriteIndexedPluginValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "value", "index", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandWriteIndexedPluginValuePayload(decoded)
	return nil
}
func (value CommandWriteIndexedPluginValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandWriteIndexedPluginValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemovePluginValuePayload struct {
	Key              string `json:"key"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemovePluginValuePayload() CommandRemovePluginValuePayload {
	return CommandRemovePluginValuePayload{
		Key:              "",
		ExpectedRevision: 0,
	}
}
func (value *CommandRemovePluginValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemovePluginValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemovePluginValuePayload(decoded)
	return nil
}
func (value CommandRemovePluginValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemovePluginValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandWritePluginConversationValuePayload struct {
	Key              string          `json:"key"`
	Value            json.RawMessage `json:"value"`
	ExpectedRevision uint64          `json:"expected_revision"`
}

func DefaultCommandWritePluginConversationValuePayload() CommandWritePluginConversationValuePayload {
	return CommandWritePluginConversationValuePayload{
		Key:              "",
		Value:            json.RawMessage("null"),
		ExpectedRevision: 0,
	}
}
func (value *CommandWritePluginConversationValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandWritePluginConversationValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "value", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandWritePluginConversationValuePayload(decoded)
	return nil
}
func (value CommandWritePluginConversationValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandWritePluginConversationValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemovePluginConversationValuePayload struct {
	Key              string `json:"key"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemovePluginConversationValuePayload() CommandRemovePluginConversationValuePayload {
	return CommandRemovePluginConversationValuePayload{
		Key:              "",
		ExpectedRevision: 0,
	}
}
func (value *CommandRemovePluginConversationValuePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemovePluginConversationValuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemovePluginConversationValuePayload(decoded)
	return nil
}
func (value CommandRemovePluginConversationValuePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemovePluginConversationValuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandResolvePluginModelPayload struct {
	Model  string         `json:"model"`
	Effort *Effort        `json:"effort"`
	Config *SessionConfig `json:"config"`
}

func DefaultCommandResolvePluginModelPayload() CommandResolvePluginModelPayload {
	return CommandResolvePluginModelPayload{
		Model:  "",
		Effort: nil,
		Config: nil,
	}
}
func (value *CommandResolvePluginModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResolvePluginModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"model"}, false); err != nil {
		return err
	}
	*value = CommandResolvePluginModelPayload(decoded)
	return nil
}
func (value CommandResolvePluginModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResolvePluginModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRequestProviderHttpPayload struct {
	Provider         ProviderID        `json:"provider"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Request          string            `json:"request"`
	Input            map[string]string `json:"input"`
}

func DefaultCommandRequestProviderHttpPayload() CommandRequestProviderHttpPayload {
	return CommandRequestProviderHttpPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
		Request:          "",
		Input:            map[string]string{},
	}
}
func (value *CommandRequestProviderHttpPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRequestProviderHttpPayload
	decoded := plain{}
	decoded.Input = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision", "request"}, false); err != nil {
		return err
	}
	*value = CommandRequestProviderHttpPayload(decoded)
	return nil
}
func (value CommandRequestProviderHttpPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRequestProviderHttpPayload
	encoded := plain(value)
	encoded.Input = normalizeCollections(encoded.Input)
	return json.Marshal(encoded)
}

type CommandExecuteProviderHttpPayload struct {
	Provider         ProviderID        `json:"provider"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Request          string            `json:"request"`
	Input            map[string]string `json:"input"`
}

func DefaultCommandExecuteProviderHttpPayload() CommandExecuteProviderHttpPayload {
	return CommandExecuteProviderHttpPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
		Request:          "",
		Input:            map[string]string{},
	}
}
func (value *CommandExecuteProviderHttpPayload) UnmarshalJSON(data []byte) error {
	type plain CommandExecuteProviderHttpPayload
	decoded := plain{}
	decoded.Input = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision", "request"}, false); err != nil {
		return err
	}
	*value = CommandExecuteProviderHttpPayload(decoded)
	return nil
}
func (value CommandExecuteProviderHttpPayload) MarshalJSON() ([]byte, error) {
	type plain CommandExecuteProviderHttpPayload
	encoded := plain(value)
	encoded.Input = normalizeCollections(encoded.Input)
	return json.Marshal(encoded)
}

type CommandCancelWebFetchPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelWebFetchPayload() CommandCancelWebFetchPayload {
	return CommandCancelWebFetchPayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelWebFetchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelWebFetchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelWebFetchPayload(decoded)
	return nil
}
func (value CommandCancelWebFetchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelWebFetchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelWebSearchPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelWebSearchPayload() CommandCancelWebSearchPayload {
	return CommandCancelWebSearchPayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelWebSearchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelWebSearchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelWebSearchPayload(decoded)
	return nil
}
func (value CommandCancelWebSearchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelWebSearchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandTestSearchDriverPayload struct {
	Driver SearchDriver `json:"driver"`
}

func DefaultCommandTestSearchDriverPayload() CommandTestSearchDriverPayload {
	return CommandTestSearchDriverPayload{
		Driver: SearchDriver(""),
	}
}
func (value *CommandTestSearchDriverPayload) UnmarshalJSON(data []byte) error {
	type plain CommandTestSearchDriverPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"driver"}, false); err != nil {
		return err
	}
	*value = CommandTestSearchDriverPayload(decoded)
	return nil
}
func (value CommandTestSearchDriverPayload) MarshalJSON() ([]byte, error) {
	type plain CommandTestSearchDriverPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandGeneratePluginTextPayload struct {
	Prompt string  `json:"prompt"`
	Model  string  `json:"model"`
	Effort *Effort `json:"effort,omitempty"`
}

func DefaultCommandGeneratePluginTextPayload() CommandGeneratePluginTextPayload {
	return CommandGeneratePluginTextPayload{
		Prompt: "",
		Model:  "",
		Effort: nil,
	}
}
func (value *CommandGeneratePluginTextPayload) UnmarshalJSON(data []byte) error {
	type plain CommandGeneratePluginTextPayload
	decoded := plain{}
	decoded.Effort = nil
	if err := DecodeObject(data, &decoded, []string{"prompt", "model"}, false); err != nil {
		return err
	}
	*value = CommandGeneratePluginTextPayload(decoded)
	return nil
}
func (value CommandGeneratePluginTextPayload) MarshalJSON() ([]byte, error) {
	type plain CommandGeneratePluginTextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandBrowseFilesPayload struct {
	Directory *string          `json:"directory"`
	After     *DirectoryCursor `json:"after"`
}

func DefaultCommandBrowseFilesPayload() CommandBrowseFilesPayload {
	return CommandBrowseFilesPayload{
		Directory: nil,
		After:     nil,
	}
}
func (value *CommandBrowseFilesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandBrowseFilesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = CommandBrowseFilesPayload(decoded)
	return nil
}
func (value CommandBrowseFilesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandBrowseFilesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandOpenPortPayload struct {
	Port uint16 `json:"port"`
}

func DefaultCommandOpenPortPayload() CommandOpenPortPayload {
	return CommandOpenPortPayload{
		Port: 0,
	}
}
func (value *CommandOpenPortPayload) UnmarshalJSON(data []byte) error {
	type plain CommandOpenPortPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"port"}, false); err != nil {
		return err
	}
	*value = CommandOpenPortPayload(decoded)
	return nil
}
func (value CommandOpenPortPayload) MarshalJSON() ([]byte, error) {
	type plain CommandOpenPortPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelPortPayload struct {
	Stream StreamID `json:"stream"`
}

func DefaultCommandCancelPortPayload() CommandCancelPortPayload {
	return CommandCancelPortPayload{
		Stream: StreamID{},
	}
}
func (value *CommandCancelPortPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelPortPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream"}, false); err != nil {
		return err
	}
	*value = CommandCancelPortPayload(decoded)
	return nil
}
func (value CommandCancelPortPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelPortPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSaveDatabasePayload struct {
	Profile          DatabaseProfile `json:"profile"`
	ExpectedRevision uint64          `json:"expected_revision"`
	Password         *Secret         `json:"password"`
}

func DefaultCommandSaveDatabasePayload() CommandSaveDatabasePayload {
	return CommandSaveDatabasePayload{
		Profile:          DefaultDatabaseProfile(),
		ExpectedRevision: 0,
		Password:         nil,
	}
}
func (value *CommandSaveDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandSaveDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSaveDatabasePayload(decoded)
	return nil
}
func (value CommandSaveDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandSaveDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveDatabasePayload struct {
	Profile          DatabaseID `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandRemoveDatabasePayload() CommandRemoveDatabasePayload {
	return CommandRemoveDatabasePayload{
		Profile:          DatabaseID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveDatabasePayload(decoded)
	return nil
}
func (value CommandRemoveDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandTestDatabasePayload struct {
	Profile          DatabaseProfile `json:"profile"`
	ExpectedRevision uint64          `json:"expected_revision"`
	Password         *Secret         `json:"password"`
}

func DefaultCommandTestDatabasePayload() CommandTestDatabasePayload {
	return CommandTestDatabasePayload{
		Profile:          DefaultDatabaseProfile(),
		ExpectedRevision: 0,
		Password:         nil,
	}
}
func (value *CommandTestDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandTestDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandTestDatabasePayload(decoded)
	return nil
}
func (value CommandTestDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandTestDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCheckDatabasePayload struct {
	Profile          DatabaseID `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandCheckDatabasePayload() CommandCheckDatabasePayload {
	return CommandCheckDatabasePayload{
		Profile:          DatabaseID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandCheckDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandCheckDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandCheckDatabasePayload(decoded)
	return nil
}
func (value CommandCheckDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandCheckDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandBrowseDatabasePayload struct {
	Profile          DatabaseID `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Database         *string    `json:"database"`
}

func DefaultCommandBrowseDatabasePayload() CommandBrowseDatabasePayload {
	return CommandBrowseDatabasePayload{
		Profile:          DatabaseID{},
		ExpectedRevision: 0,
		Database:         nil,
	}
}
func (value *CommandBrowseDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandBrowseDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandBrowseDatabasePayload(decoded)
	return nil
}
func (value CommandBrowseDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandBrowseDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandQueryDatabasePayload struct {
	ReadOnly         bool       `json:"read_only"`
	Database         *string    `json:"database"`
	Profile          DatabaseID `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Sql              string     `json:"sql"`
	RowLimit         uint32     `json:"row_limit"`
	TimeoutMs        uint64     `json:"timeout_ms"`
}

func DefaultCommandQueryDatabasePayload() CommandQueryDatabasePayload {
	return CommandQueryDatabasePayload{
		ReadOnly:         false,
		Database:         nil,
		Profile:          DatabaseID{},
		ExpectedRevision: 0,
		Sql:              "",
		RowLimit:         0,
		TimeoutMs:        0,
	}
}
func (value *CommandQueryDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandQueryDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"read_only", "profile", "expected_revision", "sql", "row_limit", "timeout_ms"}, false); err != nil {
		return err
	}
	*value = CommandQueryDatabasePayload(decoded)
	return nil
}
func (value CommandQueryDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandQueryDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelDatabasePayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelDatabasePayload() CommandCancelDatabasePayload {
	return CommandCancelDatabasePayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelDatabasePayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelDatabasePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelDatabasePayload(decoded)
	return nil
}
func (value CommandCancelDatabasePayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelDatabasePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandModifySshFilePayload struct {
	Profile          SshID         `json:"profile"`
	ExpectedRevision uint64        `json:"expected_revision"`
	Path             string        `json:"path"`
	Action           SshFileAction `json:"action"`
}

func DefaultCommandModifySshFilePayload() CommandModifySshFilePayload {
	return CommandModifySshFilePayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Path:             "",
		Action:           SshFileAction{},
	}
}
func (value *CommandModifySshFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandModifySshFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "path", "action"}, false); err != nil {
		return err
	}
	*value = CommandModifySshFilePayload(decoded)
	return nil
}
func (value CommandModifySshFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandModifySshFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandBrowseSshDirectoryPayload struct {
	Profile          SshID      `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Path             string     `json:"path"`
	After            *SshCursor `json:"after"`
}

func DefaultCommandBrowseSshDirectoryPayload() CommandBrowseSshDirectoryPayload {
	return CommandBrowseSshDirectoryPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Path:             "",
		After:            nil,
	}
}
func (value *CommandBrowseSshDirectoryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandBrowseSshDirectoryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "path"}, false); err != nil {
		return err
	}
	*value = CommandBrowseSshDirectoryPayload(decoded)
	return nil
}
func (value CommandBrowseSshDirectoryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandBrowseSshDirectoryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDownloadSshFilePayload struct {
	Profile          SshID  `json:"profile"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Path             string `json:"path"`
}

func DefaultCommandDownloadSshFilePayload() CommandDownloadSshFilePayload {
	return CommandDownloadSshFilePayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Path:             "",
	}
}
func (value *CommandDownloadSshFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandDownloadSshFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "path"}, false); err != nil {
		return err
	}
	*value = CommandDownloadSshFilePayload(decoded)
	return nil
}
func (value CommandDownloadSshFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandDownloadSshFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandFinishSshUploadPayload struct {
	Profile          SshID    `json:"profile"`
	ExpectedRevision uint64   `json:"expected_revision"`
	Path             string   `json:"path"`
	Stream           StreamID `json:"stream"`
}

func DefaultCommandFinishSshUploadPayload() CommandFinishSshUploadPayload {
	return CommandFinishSshUploadPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Path:             "",
		Stream:           StreamID{},
	}
}
func (value *CommandFinishSshUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandFinishSshUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "path", "stream"}, false); err != nil {
		return err
	}
	*value = CommandFinishSshUploadPayload(decoded)
	return nil
}
func (value CommandFinishSshUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandFinishSshUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListSshTerminalsPayload struct {
	Profile SshID `json:"profile"`
}

func DefaultCommandListSshTerminalsPayload() CommandListSshTerminalsPayload {
	return CommandListSshTerminalsPayload{
		Profile: SshID{},
	}
}
func (value *CommandListSshTerminalsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListSshTerminalsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile"}, false); err != nil {
		return err
	}
	*value = CommandListSshTerminalsPayload(decoded)
	return nil
}
func (value CommandListSshTerminalsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListSshTerminalsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSaveSshPayload struct {
	Profile          SshProfile     `json:"profile"`
	ExpectedRevision uint64         `json:"expected_revision"`
	Credential       *SshCredential `json:"credential"`
}

func DefaultCommandSaveSshPayload() CommandSaveSshPayload {
	return CommandSaveSshPayload{
		Profile:          DefaultSshProfile(),
		ExpectedRevision: 0,
		Credential:       nil,
	}
}
func (value *CommandSaveSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSaveSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSaveSshPayload(decoded)
	return nil
}
func (value CommandSaveSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSaveSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveSshPayload struct {
	Profile          SshID  `json:"profile"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemoveSshPayload() CommandRemoveSshPayload {
	return CommandRemoveSshPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveSshPayload(decoded)
	return nil
}
func (value CommandRemoveSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandTrustSshPayload struct {
	Profile          SshID      `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Key              SshHostKey `json:"key"`
}

func DefaultCommandTrustSshPayload() CommandTrustSshPayload {
	return CommandTrustSshPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Key:              DefaultSshHostKey(),
	}
}
func (value *CommandTrustSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandTrustSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "key"}, false); err != nil {
		return err
	}
	*value = CommandTrustSshPayload(decoded)
	return nil
}
func (value CommandTrustSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandTrustSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallWslHostPayload struct {
	Distribution string            `json:"distribution"`
	Network      *NetworkSelection `json:"network,omitempty"`
}

func DefaultCommandInstallWslHostPayload() CommandInstallWslHostPayload {
	return CommandInstallWslHostPayload{
		Distribution: "",
		Network:      nil,
	}
}
func (value *CommandInstallWslHostPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallWslHostPayload
	decoded := plain{}
	decoded.Network = nil
	if err := DecodeObject(data, &decoded, []string{"distribution"}, false); err != nil {
		return err
	}
	*value = CommandInstallWslHostPayload(decoded)
	return nil
}
func (value CommandInstallWslHostPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallWslHostPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadWslHostInstallPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandReadWslHostInstallPayload() CommandReadWslHostInstallPayload {
	return CommandReadWslHostInstallPayload{
		Request: RequestID{},
	}
}
func (value *CommandReadWslHostInstallPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadWslHostInstallPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandReadWslHostInstallPayload(decoded)
	return nil
}
func (value CommandReadWslHostInstallPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadWslHostInstallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelWslHostInstallPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelWslHostInstallPayload() CommandCancelWslHostInstallPayload {
	return CommandCancelWslHostInstallPayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelWslHostInstallPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelWslHostInstallPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelWslHostInstallPayload(decoded)
	return nil
}
func (value CommandCancelWslHostInstallPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelWslHostInstallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallHostPayload struct {
	Profile          SshID             `json:"profile"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Network          *NetworkSelection `json:"network,omitempty"`
}

func DefaultCommandInstallHostPayload() CommandInstallHostPayload {
	return CommandInstallHostPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Network:          nil,
	}
}
func (value *CommandInstallHostPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallHostPayload
	decoded := plain{}
	decoded.Network = nil
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallHostPayload(decoded)
	return nil
}
func (value CommandInstallHostPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallHostPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUpdateSshHostPayload struct {
	Profile          SshID  `json:"profile"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Node             NodeID `json:"node"`
}

func DefaultCommandUpdateSshHostPayload() CommandUpdateSshHostPayload {
	return CommandUpdateSshHostPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Node:             NodeID{},
	}
}
func (value *CommandUpdateSshHostPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUpdateSshHostPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "node"}, false); err != nil {
		return err
	}
	*value = CommandUpdateSshHostPayload(decoded)
	return nil
}
func (value CommandUpdateSshHostPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUpdateSshHostPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadHostInstallPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandReadHostInstallPayload() CommandReadHostInstallPayload {
	return CommandReadHostInstallPayload{
		Request: RequestID{},
	}
}
func (value *CommandReadHostInstallPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadHostInstallPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandReadHostInstallPayload(decoded)
	return nil
}
func (value CommandReadHostInstallPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadHostInstallPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCheckSshPayload struct {
	Profile          SshID  `json:"profile"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandCheckSshPayload() CommandCheckSshPayload {
	return CommandCheckSshPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandCheckSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCheckSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandCheckSshPayload(decoded)
	return nil
}
func (value CommandCheckSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCheckSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandOpenSshTerminalPayload struct {
	Profile          SshID             `json:"profile"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Launch           SshTerminalLaunch `json:"launch"`
}

func DefaultCommandOpenSshTerminalPayload() CommandOpenSshTerminalPayload {
	return CommandOpenSshTerminalPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Launch:           DefaultSshTerminalLaunch(),
	}
}
func (value *CommandOpenSshTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandOpenSshTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "launch"}, false); err != nil {
		return err
	}
	*value = CommandOpenSshTerminalPayload(decoded)
	return nil
}
func (value CommandOpenSshTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandOpenSshTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCloseSshTerminalPayload struct {
	Terminal TerminalID `json:"terminal"`
}

func DefaultCommandCloseSshTerminalPayload() CommandCloseSshTerminalPayload {
	return CommandCloseSshTerminalPayload{
		Terminal: TerminalID{},
	}
}
func (value *CommandCloseSshTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCloseSshTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal"}, false); err != nil {
		return err
	}
	*value = CommandCloseSshTerminalPayload(decoded)
	return nil
}
func (value CommandCloseSshTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCloseSshTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRunSshPayload struct {
	Profile          SshID  `json:"profile"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Command          string `json:"command"`
	TimeoutMs        uint64 `json:"timeout_ms"`
}

func DefaultCommandRunSshPayload() CommandRunSshPayload {
	return CommandRunSshPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Command:          "",
		TimeoutMs:        0,
	}
}
func (value *CommandRunSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRunSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "command", "timeout_ms"}, false); err != nil {
		return err
	}
	*value = CommandRunSshPayload(decoded)
	return nil
}
func (value CommandRunSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRunSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelSshPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelSshPayload() CommandCancelSshPayload {
	return CommandCancelSshPayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelSshPayload(decoded)
	return nil
}
func (value CommandCancelSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandTransferSshPayload struct {
	Profile          SshID       `json:"profile"`
	ExpectedRevision uint64      `json:"expected_revision"`
	Transfer         SshTransfer `json:"transfer"`
	TimeoutMs        uint64      `json:"timeout_ms"`
}

func DefaultCommandTransferSshPayload() CommandTransferSshPayload {
	return CommandTransferSshPayload{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Transfer:         DefaultSshTransfer(),
		TimeoutMs:        0,
	}
}
func (value *CommandTransferSshPayload) UnmarshalJSON(data []byte) error {
	type plain CommandTransferSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "transfer", "timeout_ms"}, false); err != nil {
		return err
	}
	*value = CommandTransferSshPayload(decoded)
	return nil
}
func (value CommandTransferSshPayload) MarshalJSON() ([]byte, error) {
	type plain CommandTransferSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSearchSkillCatalogPayload struct {
	Source PluginSkillcatalogSource `json:"source"`
	Query  string                   `json:"query"`
	Page   uint32                   `json:"page"`
	Force  bool                     `json:"force"`
}

func DefaultCommandSearchSkillCatalogPayload() CommandSearchSkillCatalogPayload {
	return CommandSearchSkillCatalogPayload{
		Source: PluginSkillcatalogSource(""),
		Query:  "",
		Page:   0,
		Force:  false,
	}
}
func (value *CommandSearchSkillCatalogPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSearchSkillCatalogPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "query", "page", "force"}, false); err != nil {
		return err
	}
	*value = CommandSearchSkillCatalogPayload(decoded)
	return nil
}
func (value CommandSearchSkillCatalogPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSearchSkillCatalogPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSearchPluginCatalogPayload struct {
	Source PluginCatalogSource `json:"source"`
	Query  string              `json:"query"`
	Page   uint32              `json:"page"`
	Force  bool                `json:"force"`
}

func DefaultCommandSearchPluginCatalogPayload() CommandSearchPluginCatalogPayload {
	return CommandSearchPluginCatalogPayload{
		Source: PluginCatalogSource(""),
		Query:  "",
		Page:   0,
		Force:  false,
	}
}
func (value *CommandSearchPluginCatalogPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSearchPluginCatalogPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "query", "page", "force"}, false); err != nil {
		return err
	}
	*value = CommandSearchPluginCatalogPayload(decoded)
	return nil
}
func (value CommandSearchPluginCatalogPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSearchPluginCatalogPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadCatalogPluginPayload struct {
	Source PluginCatalogSource `json:"source"`
	ID     string              `json:"id"`
}

func DefaultCommandReadCatalogPluginPayload() CommandReadCatalogPluginPayload {
	return CommandReadCatalogPluginPayload{
		Source: PluginCatalogSource(""),
		ID:     "",
	}
}
func (value *CommandReadCatalogPluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadCatalogPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "id"}, false); err != nil {
		return err
	}
	*value = CommandReadCatalogPluginPayload(decoded)
	return nil
}
func (value CommandReadCatalogPluginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadCatalogPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadCatalogPluginInfoPayload struct {
	Source  PluginCatalogSource `json:"source"`
	ID      string              `json:"id"`
	Bundled bool                `json:"bundled"`
}

func DefaultCommandReadCatalogPluginInfoPayload() CommandReadCatalogPluginInfoPayload {
	return CommandReadCatalogPluginInfoPayload{
		Source:  PluginCatalogSource(""),
		ID:      "",
		Bundled: false,
	}
}
func (value *CommandReadCatalogPluginInfoPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadCatalogPluginInfoPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "id", "bundled"}, false); err != nil {
		return err
	}
	*value = CommandReadCatalogPluginInfoPayload(decoded)
	return nil
}
func (value CommandReadCatalogPluginInfoPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadCatalogPluginInfoPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInspectPluginSourcePayload struct {
	Source PluginSkillsSource `json:"source"`
}

func DefaultCommandInspectPluginSourcePayload() CommandInspectPluginSourcePayload {
	return CommandInspectPluginSourcePayload{
		Source: DefaultPluginSkillsSource(),
	}
}
func (value *CommandInspectPluginSourcePayload) UnmarshalJSON(data []byte) error {
	type plain CommandInspectPluginSourcePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source"}, false); err != nil {
		return err
	}
	*value = CommandInspectPluginSourcePayload(decoded)
	return nil
}
func (value CommandInspectPluginSourcePayload) MarshalJSON() ([]byte, error) {
	type plain CommandInspectPluginSourcePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCheckPluginUpdatePayload struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Force            bool   `json:"force"`
}

func DefaultCommandCheckPluginUpdatePayload() CommandCheckPluginUpdatePayload {
	return CommandCheckPluginUpdatePayload{
		Name:             "",
		ExpectedRevision: 0,
		Force:            false,
	}
}
func (value *CommandCheckPluginUpdatePayload) UnmarshalJSON(data []byte) error {
	type plain CommandCheckPluginUpdatePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision", "force"}, false); err != nil {
		return err
	}
	*value = CommandCheckPluginUpdatePayload(decoded)
	return nil
}
func (value CommandCheckPluginUpdatePayload) MarshalJSON() ([]byte, error) {
	type plain CommandCheckPluginUpdatePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCheckPluginUpdatesPayload struct {
	Packages []PluginUpdatesRequest `json:"packages"`
	Force    bool                   `json:"force"`
}

func DefaultCommandCheckPluginUpdatesPayload() CommandCheckPluginUpdatesPayload {
	return CommandCheckPluginUpdatesPayload{
		Packages: []PluginUpdatesRequest{},
		Force:    false,
	}
}
func (value *CommandCheckPluginUpdatesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCheckPluginUpdatesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"packages", "force"}, false); err != nil {
		return err
	}
	*value = CommandCheckPluginUpdatesPayload(decoded)
	return nil
}
func (value CommandCheckPluginUpdatesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCheckPluginUpdatesPayload
	encoded := plain(value)
	encoded.Packages = normalizeCollections(encoded.Packages)
	return json.Marshal(encoded)
}

type CommandDismissInstallationPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandDismissInstallationPayload() CommandDismissInstallationPayload {
	return CommandDismissInstallationPayload{
		Request: RequestID{},
	}
}
func (value *CommandDismissInstallationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDismissInstallationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandDismissInstallationPayload(decoded)
	return nil
}
func (value CommandDismissInstallationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDismissInstallationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadInstallationsPayload struct {
	Requests []RequestID `json:"requests"`
}

func DefaultCommandReadInstallationsPayload() CommandReadInstallationsPayload {
	return CommandReadInstallationsPayload{
		Requests: []RequestID{},
	}
}
func (value *CommandReadInstallationsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadInstallationsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"requests"}, false); err != nil {
		return err
	}
	*value = CommandReadInstallationsPayload(decoded)
	return nil
}
func (value CommandReadInstallationsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadInstallationsPayload
	encoded := plain(value)
	encoded.Requests = normalizeCollections(encoded.Requests)
	return json.Marshal(encoded)
}

type CommandInstallPluginSourcePayload struct {
	Source           PluginSkillsResolved `json:"source"`
	Path             string               `json:"path"`
	Name             string               `json:"name"`
	ExpectedRevision uint64               `json:"expected_revision"`
}

func DefaultCommandInstallPluginSourcePayload() CommandInstallPluginSourcePayload {
	return CommandInstallPluginSourcePayload{
		Source:           DefaultPluginSkillsResolved(),
		Path:             "",
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandInstallPluginSourcePayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallPluginSourcePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path", "name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallPluginSourcePayload(decoded)
	return nil
}
func (value CommandInstallPluginSourcePayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallPluginSourcePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallBundledPluginPayload struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandInstallBundledPluginPayload() CommandInstallBundledPluginPayload {
	return CommandInstallBundledPluginPayload{
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandInstallBundledPluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallBundledPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallBundledPluginPayload(decoded)
	return nil
}
func (value CommandInstallBundledPluginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallBundledPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadSkillDocumentPayload struct {
	Source PluginSkillsDocumentSource `json:"source"`
}

func DefaultCommandReadSkillDocumentPayload() CommandReadSkillDocumentPayload {
	return CommandReadSkillDocumentPayload{
		Source: PluginSkillsDocumentSource{},
	}
}
func (value *CommandReadSkillDocumentPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadSkillDocumentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source"}, false); err != nil {
		return err
	}
	*value = CommandReadSkillDocumentPayload(decoded)
	return nil
}
func (value CommandReadSkillDocumentPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadSkillDocumentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDiscoverSkillsPayload struct {
	Source PluginSkillsSource `json:"source"`
}

func DefaultCommandDiscoverSkillsPayload() CommandDiscoverSkillsPayload {
	return CommandDiscoverSkillsPayload{
		Source: DefaultPluginSkillsSource(),
	}
}
func (value *CommandDiscoverSkillsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDiscoverSkillsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source"}, false); err != nil {
		return err
	}
	*value = CommandDiscoverSkillsPayload(decoded)
	return nil
}
func (value CommandDiscoverSkillsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDiscoverSkillsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDiscoverSkillUploadPayload struct {
	Stream StreamID                  `json:"stream"`
	Format PluginSkillsArchiveFormat `json:"format"`
	Source PluginUploadSource        `json:"source"`
}

func DefaultCommandDiscoverSkillUploadPayload() CommandDiscoverSkillUploadPayload {
	return CommandDiscoverSkillUploadPayload{
		Stream: StreamID{},
		Format: PluginSkillsArchiveFormat(""),
		Source: PluginUploadSource(""),
	}
}
func (value *CommandDiscoverSkillUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDiscoverSkillUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "format", "source"}, false); err != nil {
		return err
	}
	*value = CommandDiscoverSkillUploadPayload(decoded)
	return nil
}
func (value CommandDiscoverSkillUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDiscoverSkillUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDiscoverLocalSkillsPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandDiscoverLocalSkillsPayload() CommandDiscoverLocalSkillsPayload {
	return CommandDiscoverLocalSkillsPayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandDiscoverLocalSkillsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDiscoverLocalSkillsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandDiscoverLocalSkillsPayload(decoded)
	return nil
}
func (value CommandDiscoverLocalSkillsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDiscoverLocalSkillsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallSkillUploadPayload struct {
	Stream           StreamID             `json:"stream"`
	Source           PluginSkillsImported `json:"source"`
	Name             string               `json:"name"`
	ExpectedRevision uint64               `json:"expected_revision"`
}

func DefaultCommandInstallSkillUploadPayload() CommandInstallSkillUploadPayload {
	return CommandInstallSkillUploadPayload{
		Stream:           StreamID{},
		Source:           DefaultPluginSkillsImported(),
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandInstallSkillUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallSkillUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "source", "name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallSkillUploadPayload(decoded)
	return nil
}
func (value CommandInstallSkillUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallSkillUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDiscoverRegistrySkillPayload struct {
	Source PluginSkillsRegistrySource `json:"source"`
}

func DefaultCommandDiscoverRegistrySkillPayload() CommandDiscoverRegistrySkillPayload {
	return CommandDiscoverRegistrySkillPayload{
		Source: DefaultPluginSkillsRegistrySource(),
	}
}
func (value *CommandDiscoverRegistrySkillPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDiscoverRegistrySkillPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source"}, false); err != nil {
		return err
	}
	*value = CommandDiscoverRegistrySkillPayload(decoded)
	return nil
}
func (value CommandDiscoverRegistrySkillPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDiscoverRegistrySkillPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallRegistrySkillPayload struct {
	Source           PluginSkillsRegistryResolved `json:"source"`
	Name             string                       `json:"name"`
	ExpectedRevision uint64                       `json:"expected_revision"`
}

func DefaultCommandInstallRegistrySkillPayload() CommandInstallRegistrySkillPayload {
	return CommandInstallRegistrySkillPayload{
		Source:           DefaultPluginSkillsRegistryResolved(),
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandInstallRegistrySkillPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallRegistrySkillPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallRegistrySkillPayload(decoded)
	return nil
}
func (value CommandInstallRegistrySkillPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallRegistrySkillPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallSkillPayload struct {
	Source           PluginSkillsResolved `json:"source"`
	Path             string               `json:"path"`
	Name             string               `json:"name"`
	ExpectedRevision uint64               `json:"expected_revision"`
}

func DefaultCommandInstallSkillPayload() CommandInstallSkillPayload {
	return CommandInstallSkillPayload{
		Source:           DefaultPluginSkillsResolved(),
		Path:             "",
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandInstallSkillPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallSkillPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "path", "name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallSkillPayload(decoded)
	return nil
}
func (value CommandInstallSkillPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallSkillPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallMcpPayload struct {
	Name             string                                `json:"name"`
	ExpectedRevision uint64                                `json:"expected_revision"`
	Definition       PluginMcpDefinition                   `json:"definition"`
	Secrets          map[string]PluginSettingsSecretUpdate `json:"secrets"`
}

func DefaultCommandInstallMcpPayload() CommandInstallMcpPayload {
	return CommandInstallMcpPayload{
		Name:             "",
		ExpectedRevision: 0,
		Definition:       PluginMcpDefinition{},
		Secrets:          map[string]PluginSettingsSecretUpdate{},
	}
}
func (value *CommandInstallMcpPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallMcpPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision", "definition", "secrets"}, false); err != nil {
		return err
	}
	*value = CommandInstallMcpPayload(decoded)
	return nil
}
func (value CommandInstallMcpPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallMcpPayload
	encoded := plain(value)
	encoded.Secrets = normalizeCollections(encoded.Secrets)
	return json.Marshal(encoded)
}

type CommandInspectPluginUploadPayload struct {
	Stream StreamID `json:"stream"`
}

func DefaultCommandInspectPluginUploadPayload() CommandInspectPluginUploadPayload {
	return CommandInspectPluginUploadPayload{
		Stream: StreamID{},
	}
}
func (value *CommandInspectPluginUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInspectPluginUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream"}, false); err != nil {
		return err
	}
	*value = CommandInspectPluginUploadPayload(decoded)
	return nil
}
func (value CommandInspectPluginUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInspectPluginUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallPluginUploadPayload struct {
	Stream           StreamID           `json:"stream"`
	Source           PluginUploadSource `json:"source"`
	Name             string             `json:"name"`
	ExpectedRevision uint64             `json:"expected_revision"`
}

func DefaultCommandInstallPluginUploadPayload() CommandInstallPluginUploadPayload {
	return CommandInstallPluginUploadPayload{
		Stream:           StreamID{},
		Source:           PluginUploadSource(""),
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandInstallPluginUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallPluginUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "source", "name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallPluginUploadPayload(decoded)
	return nil
}
func (value CommandInstallPluginUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallPluginUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginPayload struct {
	Name string `json:"name"`
}

func DefaultCommandReadPluginPayload() CommandReadPluginPayload {
	return CommandReadPluginPayload{
		Name: "",
	}
}
func (value *CommandReadPluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginPayload(decoded)
	return nil
}
func (value CommandReadPluginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginVersionPayload struct {
	Package PluginReference `json:"package"`
}

func DefaultCommandReadPluginVersionPayload() CommandReadPluginVersionPayload {
	return CommandReadPluginVersionPayload{
		Package: DefaultPluginReference(),
	}
}
func (value *CommandReadPluginVersionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginVersionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginVersionPayload(decoded)
	return nil
}
func (value CommandReadPluginVersionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginVersionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginSettingsPayload struct {
	Package PluginReference `json:"package"`
}

func DefaultCommandReadPluginSettingsPayload() CommandReadPluginSettingsPayload {
	return CommandReadPluginSettingsPayload{
		Package: DefaultPluginReference(),
	}
}
func (value *CommandReadPluginSettingsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginSettingsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginSettingsPayload(decoded)
	return nil
}
func (value CommandReadPluginSettingsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginSettingsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginMcpPayload struct {
	Package PluginReference `json:"package"`
}

func DefaultCommandReadPluginMcpPayload() CommandReadPluginMcpPayload {
	return CommandReadPluginMcpPayload{
		Package: DefaultPluginReference(),
	}
}
func (value *CommandReadPluginMcpPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginMcpPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginMcpPayload(decoded)
	return nil
}
func (value CommandReadPluginMcpPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginMcpPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetPluginMcpAllowedPayload struct {
	Package PluginReference `json:"package"`
	Server  string          `json:"server"`
	Allowed bool            `json:"allowed"`
}

func DefaultCommandSetPluginMcpAllowedPayload() CommandSetPluginMcpAllowedPayload {
	return CommandSetPluginMcpAllowedPayload{
		Package: DefaultPluginReference(),
		Server:  "",
		Allowed: false,
	}
}
func (value *CommandSetPluginMcpAllowedPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetPluginMcpAllowedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "server", "allowed"}, false); err != nil {
		return err
	}
	*value = CommandSetPluginMcpAllowedPayload(decoded)
	return nil
}
func (value CommandSetPluginMcpAllowedPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetPluginMcpAllowedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCheckPluginMcpPayload struct {
	Package PluginReference `json:"package"`
	Server  string          `json:"server"`
}

func DefaultCommandCheckPluginMcpPayload() CommandCheckPluginMcpPayload {
	return CommandCheckPluginMcpPayload{
		Package: DefaultPluginReference(),
		Server:  "",
	}
}
func (value *CommandCheckPluginMcpPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCheckPluginMcpPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "server"}, false); err != nil {
		return err
	}
	*value = CommandCheckPluginMcpPayload(decoded)
	return nil
}
func (value CommandCheckPluginMcpPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCheckPluginMcpPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInspectConnectorCliPayload struct {
	Package   PluginReference `json:"package"`
	Connector string          `json:"connector"`
}

func DefaultCommandInspectConnectorCliPayload() CommandInspectConnectorCliPayload {
	return CommandInspectConnectorCliPayload{
		Package:   DefaultPluginReference(),
		Connector: "",
	}
}
func (value *CommandInspectConnectorCliPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInspectConnectorCliPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "connector"}, false); err != nil {
		return err
	}
	*value = CommandInspectConnectorCliPayload(decoded)
	return nil
}
func (value CommandInspectConnectorCliPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInspectConnectorCliPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStartConnectorLoginPayload struct {
	Package          PluginReference      `json:"package"`
	Connector        string               `json:"connector"`
	Connection       *PluginConnectorID   `json:"connection"`
	ExpectedRevision *uint64              `json:"expected_revision"`
	Method           PluginConnectorKind  `json:"method"`
	Input            PluginConnectorInput `json:"input"`
}

func DefaultCommandStartConnectorLoginPayload() CommandStartConnectorLoginPayload {
	return CommandStartConnectorLoginPayload{
		Package:          DefaultPluginReference(),
		Connector:        "",
		Connection:       nil,
		ExpectedRevision: nil,
		Method:           PluginConnectorKind(""),
		Input:            DefaultPluginConnectorInput(),
	}
}
func (value *CommandStartConnectorLoginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStartConnectorLoginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "connector", "method", "input"}, false); err != nil {
		return err
	}
	*value = CommandStartConnectorLoginPayload(decoded)
	return nil
}
func (value CommandStartConnectorLoginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStartConnectorLoginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSubmitConnectorInputPayload struct {
	Attempt RequestID `json:"attempt"`
	Input   Secret    `json:"input"`
}

func DefaultCommandSubmitConnectorInputPayload() CommandSubmitConnectorInputPayload {
	return CommandSubmitConnectorInputPayload{
		Attempt: RequestID{},
		Input:   Secret{},
	}
}
func (value *CommandSubmitConnectorInputPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSubmitConnectorInputPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attempt", "input"}, false); err != nil {
		return err
	}
	*value = CommandSubmitConnectorInputPayload(decoded)
	return nil
}
func (value CommandSubmitConnectorInputPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSubmitConnectorInputPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelConnectorLoginPayload struct {
	Attempt RequestID `json:"attempt"`
}

func DefaultCommandCancelConnectorLoginPayload() CommandCancelConnectorLoginPayload {
	return CommandCancelConnectorLoginPayload{
		Attempt: RequestID{},
	}
}
func (value *CommandCancelConnectorLoginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelConnectorLoginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attempt"}, false); err != nil {
		return err
	}
	*value = CommandCancelConnectorLoginPayload(decoded)
	return nil
}
func (value CommandCancelConnectorLoginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelConnectorLoginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUpdateConnectionPayload struct {
	Connection       PluginConnectorID    `json:"connection"`
	ExpectedRevision uint64               `json:"expected_revision"`
	Input            PluginConnectorInput `json:"input"`
}

func DefaultCommandUpdateConnectionPayload() CommandUpdateConnectionPayload {
	return CommandUpdateConnectionPayload{
		Connection:       RequestID{},
		ExpectedRevision: 0,
		Input:            DefaultPluginConnectorInput(),
	}
}
func (value *CommandUpdateConnectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUpdateConnectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection", "expected_revision", "input"}, false); err != nil {
		return err
	}
	*value = CommandUpdateConnectionPayload(decoded)
	return nil
}
func (value CommandUpdateConnectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUpdateConnectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetConnectionEnabledPayload struct {
	Connection       PluginConnectorID `json:"connection"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Enabled          bool              `json:"enabled"`
}

func DefaultCommandSetConnectionEnabledPayload() CommandSetConnectionEnabledPayload {
	return CommandSetConnectionEnabledPayload{
		Connection:       RequestID{},
		ExpectedRevision: 0,
		Enabled:          false,
	}
}
func (value *CommandSetConnectionEnabledPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetConnectionEnabledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection", "expected_revision", "enabled"}, false); err != nil {
		return err
	}
	*value = CommandSetConnectionEnabledPayload(decoded)
	return nil
}
func (value CommandSetConnectionEnabledPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetConnectionEnabledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDisconnectConnectionPayload struct {
	Connection       PluginConnectorID `json:"connection"`
	ExpectedRevision uint64            `json:"expected_revision"`
}

func DefaultCommandDisconnectConnectionPayload() CommandDisconnectConnectionPayload {
	return CommandDisconnectConnectionPayload{
		Connection:       RequestID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandDisconnectConnectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDisconnectConnectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandDisconnectConnectionPayload(decoded)
	return nil
}
func (value CommandDisconnectConnectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDisconnectConnectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveConnectionPayload struct {
	Connection       PluginConnectorID `json:"connection"`
	ExpectedRevision uint64            `json:"expected_revision"`
}

func DefaultCommandRemoveConnectionPayload() CommandRemoveConnectionPayload {
	return CommandRemoveConnectionPayload{
		Connection:       RequestID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveConnectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveConnectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveConnectionPayload(decoded)
	return nil
}
func (value CommandRemoveConnectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveConnectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandConfirmConnectorConsentPayload struct {
	Connection       PluginConnectorID `json:"connection"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Accept           bool              `json:"accept"`
}

func DefaultCommandConfirmConnectorConsentPayload() CommandConfirmConnectorConsentPayload {
	return CommandConfirmConnectorConsentPayload{
		Connection:       RequestID{},
		ExpectedRevision: 0,
		Accept:           false,
	}
}
func (value *CommandConfirmConnectorConsentPayload) UnmarshalJSON(data []byte) error {
	type plain CommandConfirmConnectorConsentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection", "expected_revision", "accept"}, false); err != nil {
		return err
	}
	*value = CommandConfirmConnectorConsentPayload(decoded)
	return nil
}
func (value CommandConfirmConnectorConsentPayload) MarshalJSON() ([]byte, error) {
	type plain CommandConfirmConnectorConsentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandProbeConnectionPayload struct {
	Connection PluginConnectorID `json:"connection"`
}

func DefaultCommandProbeConnectionPayload() CommandProbeConnectionPayload {
	return CommandProbeConnectionPayload{
		Connection: RequestID{},
	}
}
func (value *CommandProbeConnectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandProbeConnectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"connection"}, false); err != nil {
		return err
	}
	*value = CommandProbeConnectionPayload(decoded)
	return nil
}
func (value CommandProbeConnectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandProbeConnectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSavePluginMcpPayload struct {
	Package       PluginReference        `json:"package"`
	Configuration PluginMcpConfiguration `json:"configuration"`
}

func DefaultCommandSavePluginMcpPayload() CommandSavePluginMcpPayload {
	return CommandSavePluginMcpPayload{
		Package:       DefaultPluginReference(),
		Configuration: DefaultPluginMcpConfiguration(),
	}
}
func (value *CommandSavePluginMcpPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSavePluginMcpPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "configuration"}, false); err != nil {
		return err
	}
	*value = CommandSavePluginMcpPayload(decoded)
	return nil
}
func (value CommandSavePluginMcpPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSavePluginMcpPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginSecretPayload struct {
	Package PluginReference `json:"package"`
	Field   string          `json:"field"`
}

func DefaultCommandReadPluginSecretPayload() CommandReadPluginSecretPayload {
	return CommandReadPluginSecretPayload{
		Package: DefaultPluginReference(),
		Field:   "",
	}
}
func (value *CommandReadPluginSecretPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginSecretPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "field"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginSecretPayload(decoded)
	return nil
}
func (value CommandReadPluginSecretPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginSecretPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadPluginConnectionPayload struct {
	Package PluginReference `json:"package"`
	Card    string          `json:"card"`
}

func DefaultCommandReadPluginConnectionPayload() CommandReadPluginConnectionPayload {
	return CommandReadPluginConnectionPayload{
		Package: DefaultPluginReference(),
		Card:    "",
	}
}
func (value *CommandReadPluginConnectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginConnectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "card"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginConnectionPayload(decoded)
	return nil
}
func (value CommandReadPluginConnectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginConnectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSavePluginSettingsPayload struct {
	Package PluginReference                       `json:"package"`
	Values  map[string]json.RawMessage            `json:"values"`
	Secrets map[string]PluginSettingsSecretUpdate `json:"secrets"`
}

func DefaultCommandSavePluginSettingsPayload() CommandSavePluginSettingsPayload {
	return CommandSavePluginSettingsPayload{
		Package: DefaultPluginReference(),
		Values:  map[string]json.RawMessage{},
		Secrets: map[string]PluginSettingsSecretUpdate{},
	}
}
func (value *CommandSavePluginSettingsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSavePluginSettingsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "values", "secrets"}, false); err != nil {
		return err
	}
	*value = CommandSavePluginSettingsPayload(decoded)
	return nil
}
func (value CommandSavePluginSettingsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSavePluginSettingsPayload
	encoded := plain(value)
	encoded.Values = normalizeCollections(encoded.Values)
	encoded.Secrets = normalizeCollections(encoded.Secrets)
	return json.Marshal(encoded)
}

type CommandReadPluginViewPayload struct {
	Surface PluginDesktopSurface `json:"surface"`
	Package PluginReference      `json:"package"`
}

func DefaultCommandReadPluginViewPayload() CommandReadPluginViewPayload {
	return CommandReadPluginViewPayload{
		Surface: PluginDesktopSurface{Kind: "workspace"},
		Package: DefaultPluginReference(),
	}
}
func (value *CommandReadPluginViewPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadPluginViewPayload
	decoded := plain{}
	decoded.Surface = PluginDesktopSurface{Kind: "workspace"}
	if err := DecodeObject(data, &decoded, []string{"package"}, false); err != nil {
		return err
	}
	*value = CommandReadPluginViewPayload(decoded)
	return nil
}
func (value CommandReadPluginViewPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadPluginViewPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallPluginPayload struct {
	Worktree         WorktreeID `json:"worktree"`
	Path             string     `json:"path"`
	Name             string     `json:"name"`
	ExpectedRevision uint64     `json:"expected_revision"`
	ExpectedDigest   *string    `json:"expected_digest,omitempty"`
}

func DefaultCommandInstallPluginPayload() CommandInstallPluginPayload {
	return CommandInstallPluginPayload{
		Worktree:         WorktreeID{},
		Path:             "",
		Name:             "",
		ExpectedRevision: 0,
		ExpectedDigest:   nil,
	}
}
func (value *CommandInstallPluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallPluginPayload
	decoded := plain{}
	decoded.ExpectedDigest = nil
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandInstallPluginPayload(decoded)
	return nil
}
func (value CommandInstallPluginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallPluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetPluginEnabledPayload struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Enabled          bool   `json:"enabled"`
}

func DefaultCommandSetPluginEnabledPayload() CommandSetPluginEnabledPayload {
	return CommandSetPluginEnabledPayload{
		Name:             "",
		ExpectedRevision: 0,
		Enabled:          false,
	}
}
func (value *CommandSetPluginEnabledPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetPluginEnabledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision", "enabled"}, false); err != nil {
		return err
	}
	*value = CommandSetPluginEnabledPayload(decoded)
	return nil
}
func (value CommandSetPluginEnabledPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetPluginEnabledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemovePluginPayload struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemovePluginPayload() CommandRemovePluginPayload {
	return CommandRemovePluginPayload{
		Name:             "",
		ExpectedRevision: 0,
	}
}
func (value *CommandRemovePluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemovePluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemovePluginPayload(decoded)
	return nil
}
func (value CommandRemovePluginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemovePluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPutRolePayload struct {
	Role             RoleProfile `json:"role"`
	ExpectedRevision uint64      `json:"expected_revision"`
}

func DefaultCommandPutRolePayload() CommandPutRolePayload {
	return CommandPutRolePayload{
		Role:             DefaultRoleProfile(),
		ExpectedRevision: 0,
	}
}
func (value *CommandPutRolePayload) UnmarshalJSON(data []byte) error {
	type plain CommandPutRolePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"role", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandPutRolePayload(decoded)
	return nil
}
func (value CommandPutRolePayload) MarshalJSON() ([]byte, error) {
	type plain CommandPutRolePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveRolePayload struct {
	Role             RoleID `json:"role"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemoveRolePayload() CommandRemoveRolePayload {
	return CommandRemoveRolePayload{
		Role:             RoleID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveRolePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveRolePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"role", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveRolePayload(decoded)
	return nil
}
func (value CommandRemoveRolePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveRolePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPutTeamPayload struct {
	Team             TeamProfile `json:"team"`
	ExpectedRevision uint64      `json:"expected_revision"`
}

func DefaultCommandPutTeamPayload() CommandPutTeamPayload {
	return CommandPutTeamPayload{
		Team:             DefaultTeamProfile(),
		ExpectedRevision: 0,
	}
}
func (value *CommandPutTeamPayload) UnmarshalJSON(data []byte) error {
	type plain CommandPutTeamPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"team", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandPutTeamPayload(decoded)
	return nil
}
func (value CommandPutTeamPayload) MarshalJSON() ([]byte, error) {
	type plain CommandPutTeamPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveTeamPayload struct {
	Team             TeamID `json:"team"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemoveTeamPayload() CommandRemoveTeamPayload {
	return CommandRemoveTeamPayload{
		Team:             TeamID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveTeamPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveTeamPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"team", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveTeamPayload(decoded)
	return nil
}
func (value CommandRemoveTeamPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveTeamPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadProviderKeyPayload struct {
	Provider         ProviderID `json:"provider"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandReadProviderKeyPayload() CommandReadProviderKeyPayload {
	return CommandReadProviderKeyPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandReadProviderKeyPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadProviderKeyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandReadProviderKeyPayload(decoded)
	return nil
}
func (value CommandReadProviderKeyPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadProviderKeyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandBeginProviderLoginPayload struct {
	Provider         ProviderID `json:"provider"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Redirect         *string    `json:"redirect"`
}

func DefaultCommandBeginProviderLoginPayload() CommandBeginProviderLoginPayload {
	return CommandBeginProviderLoginPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
		Redirect:         nil,
	}
}
func (value *CommandBeginProviderLoginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandBeginProviderLoginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandBeginProviderLoginPayload(decoded)
	return nil
}
func (value CommandBeginProviderLoginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandBeginProviderLoginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCompleteProviderLoginPayload struct {
	Attempt  RequestID `json:"attempt"`
	Callback Secret    `json:"callback"`
}

func DefaultCommandCompleteProviderLoginPayload() CommandCompleteProviderLoginPayload {
	return CommandCompleteProviderLoginPayload{
		Attempt:  RequestID{},
		Callback: Secret{},
	}
}
func (value *CommandCompleteProviderLoginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCompleteProviderLoginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attempt", "callback"}, false); err != nil {
		return err
	}
	*value = CommandCompleteProviderLoginPayload(decoded)
	return nil
}
func (value CommandCompleteProviderLoginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCompleteProviderLoginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelProviderLoginPayload struct {
	Attempt RequestID `json:"attempt"`
}

func DefaultCommandCancelProviderLoginPayload() CommandCancelProviderLoginPayload {
	return CommandCancelProviderLoginPayload{
		Attempt: RequestID{},
	}
}
func (value *CommandCancelProviderLoginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelProviderLoginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attempt"}, false); err != nil {
		return err
	}
	*value = CommandCancelProviderLoginPayload(decoded)
	return nil
}
func (value CommandCancelProviderLoginPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelProviderLoginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListChannelPresetsPayload struct {
	RefreshPartners bool `json:"refresh_partners"`
}

func DefaultCommandListChannelPresetsPayload() CommandListChannelPresetsPayload {
	return CommandListChannelPresetsPayload{
		RefreshPartners: false,
	}
}
func (value *CommandListChannelPresetsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListChannelPresetsPayload
	decoded := plain{}
	decoded.RefreshPartners = false
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = CommandListChannelPresetsPayload(decoded)
	return nil
}
func (value CommandListChannelPresetsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListChannelPresetsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandValidateProviderPayload struct {
	Provider         ProviderID `json:"provider"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandValidateProviderPayload() CommandValidateProviderPayload {
	return CommandValidateProviderPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandValidateProviderPayload) UnmarshalJSON(data []byte) error {
	type plain CommandValidateProviderPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandValidateProviderPayload(decoded)
	return nil
}
func (value CommandValidateProviderPayload) MarshalJSON() ([]byte, error) {
	type plain CommandValidateProviderPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPutProviderPayload struct {
	Provider         ConversationProvider `json:"provider"`
	ExpectedRevision uint64               `json:"expected_revision"`
}

func DefaultCommandPutProviderPayload() CommandPutProviderPayload {
	return CommandPutProviderPayload{
		Provider:         DefaultConversationProvider(),
		ExpectedRevision: 0,
	}
}
func (value *CommandPutProviderPayload) UnmarshalJSON(data []byte) error {
	type plain CommandPutProviderPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandPutProviderPayload(decoded)
	return nil
}
func (value CommandPutProviderPayload) MarshalJSON() ([]byte, error) {
	type plain CommandPutProviderPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadSessionPayload struct {
	Session SessionID `json:"session"`
}

func DefaultCommandReadSessionPayload() CommandReadSessionPayload {
	return CommandReadSessionPayload{
		Session: SessionID{},
	}
}
func (value *CommandReadSessionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadSessionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = CommandReadSessionPayload(decoded)
	return nil
}
func (value CommandReadSessionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadSessionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListAssociatedSessionsPayload struct {
	Association ConversationAssociation `json:"association"`
	Before      *SessionID              `json:"before"`
	Limit       uint16                  `json:"limit"`
}

func DefaultCommandListAssociatedSessionsPayload() CommandListAssociatedSessionsPayload {
	return CommandListAssociatedSessionsPayload{
		Association: DefaultConversationAssociation(),
		Before:      nil,
		Limit:       0,
	}
}
func (value *CommandListAssociatedSessionsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListAssociatedSessionsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"association", "limit"}, false); err != nil {
		return err
	}
	*value = CommandListAssociatedSessionsPayload(decoded)
	return nil
}
func (value CommandListAssociatedSessionsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListAssociatedSessionsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadConversationPayload struct {
	Session SessionID `json:"session"`
	Before  *TurnID   `json:"before"`
	Limit   uint16    `json:"limit"`
}

func DefaultCommandReadConversationPayload() CommandReadConversationPayload {
	return CommandReadConversationPayload{
		Session: SessionID{},
		Before:  nil,
		Limit:   0,
	}
}
func (value *CommandReadConversationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadConversationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadConversationPayload(decoded)
	return nil
}
func (value CommandReadConversationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadConversationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadConversationRunsPayload struct {
	Session SessionID `json:"session"`
	Before  *TurnID   `json:"before"`
	Limit   uint16    `json:"limit"`
}

func DefaultCommandReadConversationRunsPayload() CommandReadConversationRunsPayload {
	return CommandReadConversationRunsPayload{
		Session: SessionID{},
		Before:  nil,
		Limit:   0,
	}
}
func (value *CommandReadConversationRunsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadConversationRunsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadConversationRunsPayload(decoded)
	return nil
}
func (value CommandReadConversationRunsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadConversationRunsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadConversationRunPayload struct {
	Session          SessionID `json:"session"`
	Turn             TurnID    `json:"turn"`
	ExpectedRevision *uint64   `json:"expected_revision"`
}

func DefaultCommandReadConversationRunPayload() CommandReadConversationRunPayload {
	return CommandReadConversationRunPayload{
		Session:          SessionID{},
		Turn:             TurnID{},
		ExpectedRevision: nil,
	}
}
func (value *CommandReadConversationRunPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadConversationRunPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn"}, false); err != nil {
		return err
	}
	*value = CommandReadConversationRunPayload(decoded)
	return nil
}
func (value CommandReadConversationRunPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadConversationRunPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadActivityPayload struct {
	Sessions []SessionID `json:"sessions"`
}

func DefaultCommandReadActivityPayload() CommandReadActivityPayload {
	return CommandReadActivityPayload{
		Sessions: []SessionID{},
	}
}
func (value *CommandReadActivityPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadActivityPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sessions"}, false); err != nil {
		return err
	}
	*value = CommandReadActivityPayload(decoded)
	return nil
}
func (value CommandReadActivityPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadActivityPayload
	encoded := plain(value)
	encoded.Sessions = normalizeCollections(encoded.Sessions)
	return json.Marshal(encoded)
}

type CommandReadTurnCapturePayload struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
}

func DefaultCommandReadTurnCapturePayload() CommandReadTurnCapturePayload {
	return CommandReadTurnCapturePayload{
		Session: SessionID{},
		Turn:    TurnID{},
	}
}
func (value *CommandReadTurnCapturePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadTurnCapturePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn"}, false); err != nil {
		return err
	}
	*value = CommandReadTurnCapturePayload(decoded)
	return nil
}
func (value CommandReadTurnCapturePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadTurnCapturePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadTurnPayload struct {
	Session          SessionID `json:"session"`
	Turn             TurnID    `json:"turn"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Before           *uint64   `json:"before"`
	Limit            uint16    `json:"limit"`
}

func DefaultCommandReadTurnPayload() CommandReadTurnPayload {
	return CommandReadTurnPayload{
		Session:          SessionID{},
		Turn:             TurnID{},
		ExpectedRevision: 0,
		Before:           nil,
		Limit:            0,
	}
}
func (value *CommandReadTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "expected_revision", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadTurnPayload(decoded)
	return nil
}
func (value CommandReadTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadToolDetailPayload struct {
	Session          SessionID `json:"session"`
	Turn             TurnID    `json:"turn"`
	Entry            string    `json:"entry"`
	Index            uint64    `json:"index"`
	Call             *string   `json:"call"`
	ExpectedRevision string    `json:"expected_revision"`
	Cursor           *uint64   `json:"cursor"`
	Limit            uint64    `json:"limit"`
}

func DefaultCommandReadToolDetailPayload() CommandReadToolDetailPayload {
	return CommandReadToolDetailPayload{
		Session:          SessionID{},
		Turn:             TurnID{},
		Entry:            "",
		Index:            0,
		Call:             nil,
		ExpectedRevision: "",
		Cursor:           nil,
		Limit:            0,
	}
}
func (value *CommandReadToolDetailPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadToolDetailPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "entry", "index", "expected_revision", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadToolDetailPayload(decoded)
	return nil
}
func (value CommandReadToolDetailPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadToolDetailPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListConversationAssetsPayload struct {
	Session SessionID               `json:"session"`
	Query   ConversationAssetsQuery `json:"query"`
}

func DefaultCommandListConversationAssetsPayload() CommandListConversationAssetsPayload {
	return CommandListConversationAssetsPayload{
		Session: SessionID{},
		Query:   DefaultConversationAssetsQuery(),
	}
}
func (value *CommandListConversationAssetsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListConversationAssetsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "query"}, false); err != nil {
		return err
	}
	*value = CommandListConversationAssetsPayload(decoded)
	return nil
}
func (value CommandListConversationAssetsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListConversationAssetsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSearchConversationPayload struct {
	Session SessionID               `json:"session"`
	Query   ConversationSearchQuery `json:"query"`
}

func DefaultCommandSearchConversationPayload() CommandSearchConversationPayload {
	return CommandSearchConversationPayload{
		Session: SessionID{},
		Query:   DefaultConversationSearchQuery(),
	}
}
func (value *CommandSearchConversationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSearchConversationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "query"}, false); err != nil {
		return err
	}
	*value = CommandSearchConversationPayload(decoded)
	return nil
}
func (value CommandSearchConversationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSearchConversationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListFileCheckpointsPayload struct {
	Session SessionID     `json:"session"`
	Turn    TurnID        `json:"turn"`
	Before  *CheckpointID `json:"before"`
	Limit   uint16        `json:"limit"`
}

func DefaultCommandListFileCheckpointsPayload() CommandListFileCheckpointsPayload {
	return CommandListFileCheckpointsPayload{
		Session: SessionID{},
		Turn:    TurnID{},
		Before:  nil,
		Limit:   0,
	}
}
func (value *CommandListFileCheckpointsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListFileCheckpointsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "limit"}, false); err != nil {
		return err
	}
	*value = CommandListFileCheckpointsPayload(decoded)
	return nil
}
func (value CommandListFileCheckpointsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListFileCheckpointsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadFileCheckpointPayload struct {
	Session    SessionID    `json:"session"`
	Checkpoint CheckpointID `json:"checkpoint"`
}

func DefaultCommandReadFileCheckpointPayload() CommandReadFileCheckpointPayload {
	return CommandReadFileCheckpointPayload{
		Session:    SessionID{},
		Checkpoint: CheckpointID{},
	}
}
func (value *CommandReadFileCheckpointPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadFileCheckpointPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "checkpoint"}, false); err != nil {
		return err
	}
	*value = CommandReadFileCheckpointPayload(decoded)
	return nil
}
func (value CommandReadFileCheckpointPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadFileCheckpointPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadTurnDiffPayload struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
}

func DefaultCommandReadTurnDiffPayload() CommandReadTurnDiffPayload {
	return CommandReadTurnDiffPayload{
		Session: SessionID{},
		Turn:    TurnID{},
	}
}
func (value *CommandReadTurnDiffPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadTurnDiffPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn"}, false); err != nil {
		return err
	}
	*value = CommandReadTurnDiffPayload(decoded)
	return nil
}
func (value CommandReadTurnDiffPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadTurnDiffPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadTurnHunksPayload struct {
	Session SessionID `json:"session"`
	Turn    TurnID    `json:"turn"`
	Path    string    `json:"path"`
}

func DefaultCommandReadTurnHunksPayload() CommandReadTurnHunksPayload {
	return CommandReadTurnHunksPayload{
		Session: SessionID{},
		Turn:    TurnID{},
		Path:    "",
	}
}
func (value *CommandReadTurnHunksPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadTurnHunksPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "path"}, false); err != nil {
		return err
	}
	*value = CommandReadTurnHunksPayload(decoded)
	return nil
}
func (value CommandReadTurnHunksPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadTurnHunksPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRestoreFileCheckpointPayload struct {
	Session    SessionID    `json:"session"`
	Checkpoint CheckpointID `json:"checkpoint"`
	Worktree   WorktreeID   `json:"worktree"`
}

func DefaultCommandRestoreFileCheckpointPayload() CommandRestoreFileCheckpointPayload {
	return CommandRestoreFileCheckpointPayload{
		Session:    SessionID{},
		Checkpoint: CheckpointID{},
		Worktree:   WorktreeID{},
	}
}
func (value *CommandRestoreFileCheckpointPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRestoreFileCheckpointPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "checkpoint", "worktree"}, false); err != nil {
		return err
	}
	*value = CommandRestoreFileCheckpointPayload(decoded)
	return nil
}
func (value CommandRestoreFileCheckpointPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRestoreFileCheckpointPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandForkConversationPayload struct {
	Session          SessionID `json:"session"`
	Through          TurnID    `json:"through"`
	ExpectedRevision uint64    `json:"expected_revision"`
}

func DefaultCommandForkConversationPayload() CommandForkConversationPayload {
	return CommandForkConversationPayload{
		Session:          SessionID{},
		Through:          TurnID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandForkConversationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandForkConversationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "through", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandForkConversationPayload(decoded)
	return nil
}
func (value CommandForkConversationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandForkConversationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRewindConversationPayload struct {
	Session          SessionID `json:"session"`
	Through          *TurnID   `json:"through"`
	ExpectedHead     TurnID    `json:"expected_head"`
	ExpectedRevision uint64    `json:"expected_revision"`
}

func DefaultCommandRewindConversationPayload() CommandRewindConversationPayload {
	return CommandRewindConversationPayload{
		Session:          SessionID{},
		Through:          nil,
		ExpectedHead:     TurnID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRewindConversationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRewindConversationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_head", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRewindConversationPayload(decoded)
	return nil
}
func (value CommandRewindConversationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRewindConversationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReplaceTurnPayload struct {
	Session                 SessionID         `json:"session"`
	Turn                    TurnID            `json:"turn"`
	ExpectedHead            TurnID            `json:"expected_head"`
	ExpectedHistoryRevision uint64            `json:"expected_history_revision"`
	ExpectedRevision        uint64            `json:"expected_revision"`
	Message                 ConversationInput `json:"message"`
}

func DefaultCommandReplaceTurnPayload() CommandReplaceTurnPayload {
	return CommandReplaceTurnPayload{
		Session:                 SessionID{},
		Turn:                    TurnID{},
		ExpectedHead:            TurnID{},
		ExpectedHistoryRevision: 0,
		ExpectedRevision:        0,
		Message:                 DefaultConversationInput(),
	}
}
func (value *CommandReplaceTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReplaceTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "expected_head", "expected_history_revision", "expected_revision", "message"}, false); err != nil {
		return err
	}
	*value = CommandReplaceTurnPayload(decoded)
	return nil
}
func (value CommandReplaceTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReplaceTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSaveProviderPayload struct {
	Provider         ConversationProvider `json:"provider"`
	ExpectedRevision uint64               `json:"expected_revision"`
	Secret           *Secret              `json:"secret"`
}

func DefaultCommandSaveProviderPayload() CommandSaveProviderPayload {
	return CommandSaveProviderPayload{
		Provider:         DefaultConversationProvider(),
		ExpectedRevision: 0,
		Secret:           nil,
	}
}
func (value *CommandSaveProviderPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSaveProviderPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSaveProviderPayload(decoded)
	return nil
}
func (value CommandSaveProviderPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSaveProviderPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveProviderPayload struct {
	Provider         ProviderID `json:"provider"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandRemoveProviderPayload() CommandRemoveProviderPayload {
	return CommandRemoveProviderPayload{
		Provider:         ProviderID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveProviderPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveProviderPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveProviderPayload(decoded)
	return nil
}
func (value CommandRemoveProviderPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveProviderPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSubmitTurnPayload struct {
	Session          SessionID         `json:"session"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultCommandSubmitTurnPayload() CommandSubmitTurnPayload {
	return CommandSubmitTurnPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Message:          DefaultConversationInput(),
	}
}
func (value *CommandSubmitTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSubmitTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "message"}, false); err != nil {
		return err
	}
	*value = CommandSubmitTurnPayload(decoded)
	return nil
}
func (value CommandSubmitTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSubmitTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSubmitTaskPayload struct {
	Session          SessionID         `json:"session"`
	ExpectedRevision *uint64           `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultCommandSubmitTaskPayload() CommandSubmitTaskPayload {
	return CommandSubmitTaskPayload{
		Session:          SessionID{},
		ExpectedRevision: nil,
		Message:          DefaultConversationInput(),
	}
}
func (value *CommandSubmitTaskPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSubmitTaskPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "message"}, false); err != nil {
		return err
	}
	*value = CommandSubmitTaskPayload(decoded)
	return nil
}
func (value CommandSubmitTaskPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSubmitTaskPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandGuideTurnPayload struct {
	Recipient        TurnID            `json:"recipient"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultCommandGuideTurnPayload() CommandGuideTurnPayload {
	return CommandGuideTurnPayload{
		Recipient:        TurnID{},
		ExpectedRevision: 0,
		Message:          DefaultConversationInput(),
	}
}
func (value *CommandGuideTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandGuideTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"recipient", "expected_revision", "message"}, false); err != nil {
		return err
	}
	*value = CommandGuideTurnPayload(decoded)
	return nil
}
func (value CommandGuideTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandGuideTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDeliverBackgroundPayload struct {
	Session SessionID `json:"session"`
}

func DefaultCommandDeliverBackgroundPayload() CommandDeliverBackgroundPayload {
	return CommandDeliverBackgroundPayload{
		Session: SessionID{},
	}
}
func (value *CommandDeliverBackgroundPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDeliverBackgroundPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = CommandDeliverBackgroundPayload(decoded)
	return nil
}
func (value CommandDeliverBackgroundPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDeliverBackgroundPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandContinueTurnPayload struct {
	Session          SessionID          `json:"session"`
	After            *TurnID            `json:"after"`
	Message          ConversationInput  `json:"message"`
	Key              string             `json:"key"`
	ExpectedRevision uint64             `json:"expected_revision"`
	Scope            PluginStorageScope `json:"scope"`
}

func DefaultCommandContinueTurnPayload() CommandContinueTurnPayload {
	return CommandContinueTurnPayload{
		Session:          SessionID{},
		After:            nil,
		Message:          DefaultConversationInput(),
		Key:              "",
		ExpectedRevision: 0,
		Scope:            PluginStorageScope(""),
	}
}
func (value *CommandContinueTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandContinueTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "message", "key", "expected_revision", "scope"}, false); err != nil {
		return err
	}
	*value = CommandContinueTurnPayload(decoded)
	return nil
}
func (value CommandContinueTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandContinueTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCompactContextPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Focus            *string   `json:"focus,omitempty"`
}

func DefaultCommandCompactContextPayload() CommandCompactContextPayload {
	return CommandCompactContextPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Focus:            nil,
	}
}
func (value *CommandCompactContextPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCompactContextPayload
	decoded := plain{}
	decoded.Focus = nil
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandCompactContextPayload(decoded)
	return nil
}
func (value CommandCompactContextPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCompactContextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStartQueuedTurnPayload struct {
	Turn TurnID `json:"turn"`
}

func DefaultCommandStartQueuedTurnPayload() CommandStartQueuedTurnPayload {
	return CommandStartQueuedTurnPayload{
		Turn: TurnID{},
	}
}
func (value *CommandStartQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStartQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn"}, false); err != nil {
		return err
	}
	*value = CommandStartQueuedTurnPayload(decoded)
	return nil
}
func (value CommandStartQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStartQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStopTurnPayload struct {
	Turn TurnID `json:"turn"`
}

func DefaultCommandStopTurnPayload() CommandStopTurnPayload {
	return CommandStopTurnPayload{
		Turn: TurnID{},
	}
}
func (value *CommandStopTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStopTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn"}, false); err != nil {
		return err
	}
	*value = CommandStopTurnPayload(decoded)
	return nil
}
func (value CommandStopTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStopTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRunCommandPayload struct {
	Turn        TurnID         `json:"turn"`
	Command     string         `json:"command"`
	Access      ProcessAccess  `json:"access"`
	Description *string        `json:"description"`
	Cwd         string         `json:"cwd"`
	TimeoutMs   uint64         `json:"timeout_ms"`
	Background  bool           `json:"background"`
	FollowUp    bool           `json:"follow_up,omitempty"`
	Pty         bool           `json:"pty"`
	Attachments []AttachmentID `json:"attachments"`
}

func DefaultCommandRunCommandPayload() CommandRunCommandPayload {
	return CommandRunCommandPayload{
		Turn:        TurnID{},
		Command:     "",
		Access:      ProcessAccessWorkspace,
		Description: nil,
		Cwd:         "",
		TimeoutMs:   0,
		Background:  false,
		FollowUp:    false,
		Pty:         false,
		Attachments: []AttachmentID{},
	}
}
func (value *CommandRunCommandPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRunCommandPayload
	decoded := plain{}
	decoded.FollowUp = false
	decoded.Pty = false
	if err := DecodeObject(data, &decoded, []string{"turn", "command", "access", "cwd", "timeout_ms", "background", "attachments"}, false); err != nil {
		return err
	}
	*value = CommandRunCommandPayload(decoded)
	return nil
}
func (value CommandRunCommandPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRunCommandPayload
	encoded := plain(value)
	encoded.Attachments = normalizeCollections(encoded.Attachments)
	return json.Marshal(encoded)
}

type CommandListCommandsPayload struct {
	Session SessionID `json:"session"`
}

func DefaultCommandListCommandsPayload() CommandListCommandsPayload {
	return CommandListCommandsPayload{
		Session: SessionID{},
	}
}
func (value *CommandListCommandsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListCommandsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = CommandListCommandsPayload(decoded)
	return nil
}
func (value CommandListCommandsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListCommandsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadCommandPayload struct {
	Session SessionID `json:"session"`
	ID      RequestID `json:"id"`
	WaitMs  uint64    `json:"wait_ms"`
	Until   *string   `json:"until"`
}

func DefaultCommandReadCommandPayload() CommandReadCommandPayload {
	return CommandReadCommandPayload{
		Session: SessionID{},
		ID:      RequestID{},
		WaitMs:  0,
		Until:   nil,
	}
}
func (value *CommandReadCommandPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadCommandPayload
	decoded := plain{}
	decoded.WaitMs = 0
	decoded.Until = nil
	if err := DecodeObject(data, &decoded, []string{"session", "id"}, false); err != nil {
		return err
	}
	*value = CommandReadCommandPayload(decoded)
	return nil
}
func (value CommandReadCommandPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadCommandPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadCommandPagePayload struct {
	Session SessionID                 `json:"session"`
	ID      RequestID                 `json:"id"`
	Stream  ProcessOutputOutputStream `json:"stream"`
	Offset  uint64                    `json:"offset"`
	Limit   uint64                    `json:"limit"`
	WaitMs  uint64                    `json:"wait_ms"`
	Until   *string                   `json:"until"`
}

func DefaultCommandReadCommandPagePayload() CommandReadCommandPagePayload {
	return CommandReadCommandPagePayload{
		Session: SessionID{},
		ID:      RequestID{},
		Stream:  ProcessOutputOutputStreamStdout,
		Offset:  0,
		Limit:   0,
		WaitMs:  0,
		Until:   nil,
	}
}
func (value *CommandReadCommandPagePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadCommandPagePayload
	decoded := plain{}
	decoded.WaitMs = 0
	decoded.Until = nil
	if err := DecodeObject(data, &decoded, []string{"session", "id", "stream", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadCommandPagePayload(decoded)
	return nil
}
func (value CommandReadCommandPagePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadCommandPagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadToolOutputPayload struct {
	Turn   TurnID                     `json:"turn"`
	Output ToolOutputOutputReference  `json:"output"`
	Stream *ProcessOutputOutputStream `json:"stream"`
	Offset uint64                     `json:"offset"`
	Limit  uint64                     `json:"limit"`
}

func DefaultCommandReadToolOutputPayload() CommandReadToolOutputPayload {
	return CommandReadToolOutputPayload{
		Turn:   TurnID{},
		Output: DefaultToolOutputOutputReference(),
		Stream: nil,
		Offset: 0,
		Limit:  0,
	}
}
func (value *CommandReadToolOutputPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadToolOutputPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "output", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadToolOutputPayload(decoded)
	return nil
}
func (value CommandReadToolOutputPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadToolOutputPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStopCommandsPayload struct {
	Session SessionID `json:"session"`
}

func DefaultCommandStopCommandsPayload() CommandStopCommandsPayload {
	return CommandStopCommandsPayload{
		Session: SessionID{},
	}
}
func (value *CommandStopCommandsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStopCommandsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = CommandStopCommandsPayload(decoded)
	return nil
}
func (value CommandStopCommandsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStopCommandsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStopCommandPayload struct {
	Session SessionID `json:"session"`
	ID      RequestID `json:"id"`
}

func DefaultCommandStopCommandPayload() CommandStopCommandPayload {
	return CommandStopCommandPayload{
		Session: SessionID{},
		ID:      RequestID{},
	}
}
func (value *CommandStopCommandPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStopCommandPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "id"}, false); err != nil {
		return err
	}
	*value = CommandStopCommandPayload(decoded)
	return nil
}
func (value CommandStopCommandPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStopCommandPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandWriteCommandPayload struct {
	Session SessionID `json:"session"`
	ID      RequestID `json:"id"`
	Input   string    `json:"input"`
}

func DefaultCommandWriteCommandPayload() CommandWriteCommandPayload {
	return CommandWriteCommandPayload{
		Session: SessionID{},
		ID:      RequestID{},
		Input:   "",
	}
}
func (value *CommandWriteCommandPayload) UnmarshalJSON(data []byte) error {
	type plain CommandWriteCommandPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "id", "input"}, false); err != nil {
		return err
	}
	*value = CommandWriteCommandPayload(decoded)
	return nil
}
func (value CommandWriteCommandPayload) MarshalJSON() ([]byte, error) {
	type plain CommandWriteCommandPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandResolveApprovalPayload struct {
	Session  SessionID            `json:"session"`
	Approval ApprovalID           `json:"approval"`
	Decision ConversationDecision `json:"decision"`
}

func DefaultCommandResolveApprovalPayload() CommandResolveApprovalPayload {
	return CommandResolveApprovalPayload{
		Session:  SessionID{},
		Approval: ApprovalID{},
		Decision: ConversationDecision{},
	}
}
func (value *CommandResolveApprovalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResolveApprovalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "approval", "decision"}, false); err != nil {
		return err
	}
	*value = CommandResolveApprovalPayload(decoded)
	return nil
}
func (value CommandResolveApprovalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResolveApprovalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandResolveQuestionPayload struct {
	Session  SessionID                    `json:"session"`
	Question QuestionID                   `json:"question"`
	Response ConversationQuestionResponse `json:"response"`
}

func DefaultCommandResolveQuestionPayload() CommandResolveQuestionPayload {
	return CommandResolveQuestionPayload{
		Session:  SessionID{},
		Question: QuestionID{},
		Response: ConversationQuestionResponse{},
	}
}
func (value *CommandResolveQuestionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResolveQuestionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "question", "response"}, false); err != nil {
		return err
	}
	*value = CommandResolveQuestionPayload(decoded)
	return nil
}
func (value CommandResolveQuestionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResolveQuestionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadQueuedTurnPayload struct {
	Turn TurnID `json:"turn"`
}

func DefaultCommandReadQueuedTurnPayload() CommandReadQueuedTurnPayload {
	return CommandReadQueuedTurnPayload{
		Turn: TurnID{},
	}
}
func (value *CommandReadQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn"}, false); err != nil {
		return err
	}
	*value = CommandReadQueuedTurnPayload(decoded)
	return nil
}
func (value CommandReadQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandEditQueuedTurnPayload struct {
	Turn             TurnID            `json:"turn"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultCommandEditQueuedTurnPayload() CommandEditQueuedTurnPayload {
	return CommandEditQueuedTurnPayload{
		Turn:             TurnID{},
		ExpectedRevision: 0,
		Message:          DefaultConversationInput(),
	}
}
func (value *CommandEditQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandEditQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "expected_revision", "message"}, false); err != nil {
		return err
	}
	*value = CommandEditQueuedTurnPayload(decoded)
	return nil
}
func (value CommandEditQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandEditQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveQueuedTurnPayload struct {
	Turn             TurnID `json:"turn"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandRemoveQueuedTurnPayload() CommandRemoveQueuedTurnPayload {
	return CommandRemoveQueuedTurnPayload{
		Turn:             TurnID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveQueuedTurnPayload(decoded)
	return nil
}
func (value CommandRemoveQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSendQueuedTurnPayload struct {
	Turn             TurnID `json:"turn"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandSendQueuedTurnPayload() CommandSendQueuedTurnPayload {
	return CommandSendQueuedTurnPayload{
		Turn:             TurnID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandSendQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSendQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSendQueuedTurnPayload(decoded)
	return nil
}
func (value CommandSendQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSendQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandGuideQueuedTurnPayload struct {
	Turn             TurnID `json:"turn"`
	Recipient        TurnID `json:"recipient"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandGuideQueuedTurnPayload() CommandGuideQueuedTurnPayload {
	return CommandGuideQueuedTurnPayload{
		Turn:             TurnID{},
		Recipient:        TurnID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandGuideQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandGuideQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "recipient", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandGuideQueuedTurnPayload(decoded)
	return nil
}
func (value CommandGuideQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandGuideQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandWithdrawGuidancePayload struct {
	Turn             TurnID `json:"turn"`
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandWithdrawGuidancePayload() CommandWithdrawGuidancePayload {
	return CommandWithdrawGuidancePayload{
		Turn:             TurnID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandWithdrawGuidancePayload) UnmarshalJSON(data []byte) error {
	type plain CommandWithdrawGuidancePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandWithdrawGuidancePayload(decoded)
	return nil
}
func (value CommandWithdrawGuidancePayload) MarshalJSON() ([]byte, error) {
	type plain CommandWithdrawGuidancePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMoveQueuedTurnPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Turn             TurnID    `json:"turn"`
	Before           *TurnID   `json:"before"`
}

func DefaultCommandMoveQueuedTurnPayload() CommandMoveQueuedTurnPayload {
	return CommandMoveQueuedTurnPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Turn:             TurnID{},
		Before:           nil,
	}
}
func (value *CommandMoveQueuedTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandMoveQueuedTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "turn"}, false); err != nil {
		return err
	}
	*value = CommandMoveQueuedTurnPayload(decoded)
	return nil
}
func (value CommandMoveQueuedTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandMoveQueuedTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetQueuePausedPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Paused           bool      `json:"paused"`
}

func DefaultCommandSetQueuePausedPayload() CommandSetQueuePausedPayload {
	return CommandSetQueuePausedPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Paused:           false,
	}
}
func (value *CommandSetQueuePausedPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetQueuePausedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "paused"}, false); err != nil {
		return err
	}
	*value = CommandSetQueuePausedPayload(decoded)
	return nil
}
func (value CommandSetQueuePausedPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetQueuePausedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandResetTelemetryIdentityPayload struct {
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandResetTelemetryIdentityPayload() CommandResetTelemetryIdentityPayload {
	return CommandResetTelemetryIdentityPayload{
		ExpectedRevision: 0,
	}
}
func (value *CommandResetTelemetryIdentityPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResetTelemetryIdentityPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandResetTelemetryIdentityPayload(decoded)
	return nil
}
func (value CommandResetTelemetryIdentityPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResetTelemetryIdentityPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListNetworkPresetsPayload struct {
	Refresh bool `json:"refresh"`
}

func DefaultCommandListNetworkPresetsPayload() CommandListNetworkPresetsPayload {
	return CommandListNetworkPresetsPayload{
		Refresh: false,
	}
}
func (value *CommandListNetworkPresetsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListNetworkPresetsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"refresh"}, false); err != nil {
		return err
	}
	*value = CommandListNetworkPresetsPayload(decoded)
	return nil
}
func (value CommandListNetworkPresetsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListNetworkPresetsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandProbeNetworkPayload struct {
	Selection NetworkSelection `json:"selection"`
}

func DefaultCommandProbeNetworkPayload() CommandProbeNetworkPayload {
	return CommandProbeNetworkPayload{
		Selection: NetworkSelection{},
	}
}
func (value *CommandProbeNetworkPayload) UnmarshalJSON(data []byte) error {
	type plain CommandProbeNetworkPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"selection"}, false); err != nil {
		return err
	}
	*value = CommandProbeNetworkPayload(decoded)
	return nil
}
func (value CommandProbeNetworkPayload) MarshalJSON() ([]byte, error) {
	type plain CommandProbeNetworkPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSaveNetworkSettingsPayload struct {
	ExpectedRevision uint64           `json:"expected_revision"`
	Selection        NetworkSelection `json:"selection"`
}

func DefaultCommandSaveNetworkSettingsPayload() CommandSaveNetworkSettingsPayload {
	return CommandSaveNetworkSettingsPayload{
		ExpectedRevision: 0,
		Selection:        NetworkSelection{},
	}
}
func (value *CommandSaveNetworkSettingsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSaveNetworkSettingsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected_revision", "selection"}, false); err != nil {
		return err
	}
	*value = CommandSaveNetworkSettingsPayload(decoded)
	return nil
}
func (value CommandSaveNetworkSettingsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSaveNetworkSettingsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandExportBackupPayload struct {
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandExportBackupPayload() CommandExportBackupPayload {
	return CommandExportBackupPayload{
		ExpectedRevision: 0,
	}
}
func (value *CommandExportBackupPayload) UnmarshalJSON(data []byte) error {
	type plain CommandExportBackupPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandExportBackupPayload(decoded)
	return nil
}
func (value CommandExportBackupPayload) MarshalJSON() ([]byte, error) {
	type plain CommandExportBackupPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDownloadBackupPayload struct {
	ExpectedRevision uint64 `json:"expected_revision"`
}

func DefaultCommandDownloadBackupPayload() CommandDownloadBackupPayload {
	return CommandDownloadBackupPayload{
		ExpectedRevision: 0,
	}
}
func (value *CommandDownloadBackupPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDownloadBackupPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandDownloadBackupPayload(decoded)
	return nil
}
func (value CommandDownloadBackupPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDownloadBackupPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUploadBackupPayload struct {
	Size     uint64 `json:"size"`
	Revision string `json:"revision"`
}

func DefaultCommandUploadBackupPayload() CommandUploadBackupPayload {
	return CommandUploadBackupPayload{
		Size:     0,
		Revision: "",
	}
}
func (value *CommandUploadBackupPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUploadBackupPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"size", "revision"}, false); err != nil {
		return err
	}
	*value = CommandUploadBackupPayload(decoded)
	return nil
}
func (value CommandUploadBackupPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUploadBackupPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRestoreBackupFilePayload struct {
	Stream           StreamID `json:"stream"`
	ExpectedRevision uint64   `json:"expected_revision"`
	Target           string   `json:"target"`
	Passphrase       *Secret  `json:"passphrase"`
}

func DefaultCommandRestoreBackupFilePayload() CommandRestoreBackupFilePayload {
	return CommandRestoreBackupFilePayload{
		Stream:           StreamID{},
		ExpectedRevision: 0,
		Target:           "",
		Passphrase:       nil,
	}
}
func (value *CommandRestoreBackupFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRestoreBackupFilePayload
	decoded := plain{}
	decoded.Passphrase = nil
	if err := DecodeObject(data, &decoded, []string{"stream", "expected_revision", "target"}, false); err != nil {
		return err
	}
	*value = CommandRestoreBackupFilePayload(decoded)
	return nil
}
func (value CommandRestoreBackupFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRestoreBackupFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRestoreBackupPayload struct {
	ID               string         `json:"id"`
	ExpectedRevision uint64         `json:"expected_revision"`
	Target           string         `json:"target"`
	Passphrase       *Secret        `json:"passphrase"`
	Location         BackupLocation `json:"location"`
}

func DefaultCommandRestoreBackupPayload() CommandRestoreBackupPayload {
	return CommandRestoreBackupPayload{
		ID:               "",
		ExpectedRevision: 0,
		Target:           "",
		Passphrase:       nil,
		Location:         BackupLocationOnline,
	}
}
func (value *CommandRestoreBackupPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRestoreBackupPayload
	decoded := plain{}
	decoded.Passphrase = nil
	decoded.Location = BackupLocationOnline
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision", "target"}, false); err != nil {
		return err
	}
	*value = CommandRestoreBackupPayload(decoded)
	return nil
}
func (value CommandRestoreBackupPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRestoreBackupPayload
	encoded := plain(value)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Location == BackupLocationOnline {
		delete(fields, "location")
	}
	return json.Marshal(fields)
}

type CommandReadLocalModelsPayload struct {
	Kind *ModelsKind `json:"kind"`
}

func DefaultCommandReadLocalModelsPayload() CommandReadLocalModelsPayload {
	return CommandReadLocalModelsPayload{
		Kind: nil,
	}
}
func (value *CommandReadLocalModelsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadLocalModelsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = CommandReadLocalModelsPayload(decoded)
	return nil
}
func (value CommandReadLocalModelsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadLocalModelsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRefreshLocalModelPayload struct {
	ID string `json:"id"`
}

func DefaultCommandRefreshLocalModelPayload() CommandRefreshLocalModelPayload {
	return CommandRefreshLocalModelPayload{
		ID: "",
	}
}
func (value *CommandRefreshLocalModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRefreshLocalModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = CommandRefreshLocalModelPayload(decoded)
	return nil
}
func (value CommandRefreshLocalModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRefreshLocalModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRegisterLocalModelPayload struct {
	Definition ModelsDefinition `json:"definition"`
}

func DefaultCommandRegisterLocalModelPayload() CommandRegisterLocalModelPayload {
	return CommandRegisterLocalModelPayload{
		Definition: DefaultModelsDefinition(),
	}
}
func (value *CommandRegisterLocalModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRegisterLocalModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"definition"}, false); err != nil {
		return err
	}
	*value = CommandRegisterLocalModelPayload(decoded)
	return nil
}
func (value CommandRegisterLocalModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRegisterLocalModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInstallLocalModelPayload struct {
	ID string `json:"id"`
}

func DefaultCommandInstallLocalModelPayload() CommandInstallLocalModelPayload {
	return CommandInstallLocalModelPayload{
		ID: "",
	}
}
func (value *CommandInstallLocalModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallLocalModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = CommandInstallLocalModelPayload(decoded)
	return nil
}
func (value CommandInstallLocalModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInstallLocalModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDeleteLocalModelPayload struct {
	ID string `json:"id"`
}

func DefaultCommandDeleteLocalModelPayload() CommandDeleteLocalModelPayload {
	return CommandDeleteLocalModelPayload{
		ID: "",
	}
}
func (value *CommandDeleteLocalModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDeleteLocalModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = CommandDeleteLocalModelPayload(decoded)
	return nil
}
func (value CommandDeleteLocalModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDeleteLocalModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelLocalModelPayload struct {
	ID string `json:"id"`
}

func DefaultCommandCancelLocalModelPayload() CommandCancelLocalModelPayload {
	return CommandCancelLocalModelPayload{
		ID: "",
	}
}
func (value *CommandCancelLocalModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelLocalModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = CommandCancelLocalModelPayload(decoded)
	return nil
}
func (value CommandCancelLocalModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelLocalModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadModelVersionsPayload struct {
	Hub        ModelsHub `json:"hub"`
	Repository string    `json:"repository"`
}

func DefaultCommandReadModelVersionsPayload() CommandReadModelVersionsPayload {
	return CommandReadModelVersionsPayload{
		Hub:        ModelsHubHuggingFace,
		Repository: "",
	}
}
func (value *CommandReadModelVersionsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadModelVersionsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"hub", "repository"}, false); err != nil {
		return err
	}
	*value = CommandReadModelVersionsPayload(decoded)
	return nil
}
func (value CommandReadModelVersionsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadModelVersionsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandResolveModelPayload struct {
	Kind   ModelsKind   `json:"kind"`
	Source ModelsSource `json:"source"`
}

func DefaultCommandResolveModelPayload() CommandResolveModelPayload {
	return CommandResolveModelPayload{
		Kind:   ModelsKind(""),
		Source: DefaultModelsSource(),
	}
}
func (value *CommandResolveModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResolveModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "source"}, false); err != nil {
		return err
	}
	*value = CommandResolveModelPayload(decoded)
	return nil
}
func (value CommandResolveModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResolveModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateKnowledgeCollectionPayload struct {
	Definition KnowledgeDefinition `json:"definition"`
	Plugin     *string             `json:"plugin"`
}

func DefaultCommandCreateKnowledgeCollectionPayload() CommandCreateKnowledgeCollectionPayload {
	return CommandCreateKnowledgeCollectionPayload{
		Definition: DefaultKnowledgeDefinition(),
		Plugin:     nil,
	}
}
func (value *CommandCreateKnowledgeCollectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateKnowledgeCollectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"definition"}, false); err != nil {
		return err
	}
	*value = CommandCreateKnowledgeCollectionPayload(decoded)
	return nil
}
func (value CommandCreateKnowledgeCollectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateKnowledgeCollectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUpdateKnowledgeCollectionPayload struct {
	ID               KnowledgeID         `json:"id"`
	ExpectedRevision uint64              `json:"expected_revision"`
	Definition       KnowledgeDefinition `json:"definition"`
}

func DefaultCommandUpdateKnowledgeCollectionPayload() CommandUpdateKnowledgeCollectionPayload {
	return CommandUpdateKnowledgeCollectionPayload{
		ID:               KnowledgeID{},
		ExpectedRevision: 0,
		Definition:       DefaultKnowledgeDefinition(),
	}
}
func (value *CommandUpdateKnowledgeCollectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUpdateKnowledgeCollectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision", "definition"}, false); err != nil {
		return err
	}
	*value = CommandUpdateKnowledgeCollectionPayload(decoded)
	return nil
}
func (value CommandUpdateKnowledgeCollectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUpdateKnowledgeCollectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDeleteKnowledgeCollectionPayload struct {
	ID               KnowledgeID `json:"id"`
	ExpectedRevision uint64      `json:"expected_revision"`
}

func DefaultCommandDeleteKnowledgeCollectionPayload() CommandDeleteKnowledgeCollectionPayload {
	return CommandDeleteKnowledgeCollectionPayload{
		ID:               KnowledgeID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandDeleteKnowledgeCollectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDeleteKnowledgeCollectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandDeleteKnowledgeCollectionPayload(decoded)
	return nil
}
func (value CommandDeleteKnowledgeCollectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDeleteKnowledgeCollectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRebuildKnowledgeCollectionPayload struct {
	ID               KnowledgeID `json:"id"`
	ExpectedRevision uint64      `json:"expected_revision"`
}

func DefaultCommandRebuildKnowledgeCollectionPayload() CommandRebuildKnowledgeCollectionPayload {
	return CommandRebuildKnowledgeCollectionPayload{
		ID:               KnowledgeID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRebuildKnowledgeCollectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRebuildKnowledgeCollectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRebuildKnowledgeCollectionPayload(decoded)
	return nil
}
func (value CommandRebuildKnowledgeCollectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRebuildKnowledgeCollectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPauseKnowledgeCollectionPayload struct {
	ID               KnowledgeID `json:"id"`
	ExpectedRevision uint64      `json:"expected_revision"`
	Paused           bool        `json:"paused"`
}

func DefaultCommandPauseKnowledgeCollectionPayload() CommandPauseKnowledgeCollectionPayload {
	return CommandPauseKnowledgeCollectionPayload{
		ID:               KnowledgeID{},
		ExpectedRevision: 0,
		Paused:           false,
	}
}
func (value *CommandPauseKnowledgeCollectionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandPauseKnowledgeCollectionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision", "paused"}, false); err != nil {
		return err
	}
	*value = CommandPauseKnowledgeCollectionPayload(decoded)
	return nil
}
func (value CommandPauseKnowledgeCollectionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandPauseKnowledgeCollectionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandOpenTerminalPayload struct {
	Terminal   TerminalID         `json:"terminal"`
	Viewport   TerminalViewport   `json:"viewport"`
	Appearance TerminalAppearance `json:"appearance"`
}

func DefaultCommandOpenTerminalPayload() CommandOpenTerminalPayload {
	return CommandOpenTerminalPayload{
		Terminal:   TerminalID{},
		Viewport:   DefaultTerminalViewport(),
		Appearance: DefaultTerminalAppearance(),
	}
}
func (value *CommandOpenTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandOpenTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal", "viewport", "appearance"}, false); err != nil {
		return err
	}
	*value = CommandOpenTerminalPayload(decoded)
	return nil
}
func (value CommandOpenTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandOpenTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandOpenToolTerminalPayload struct {
	Tool   TerminalToolsTool `json:"tool"`
	Launch TerminalLaunch    `json:"launch"`
}

func DefaultCommandOpenToolTerminalPayload() CommandOpenToolTerminalPayload {
	return CommandOpenToolTerminalPayload{
		Tool:   TerminalToolsTool(""),
		Launch: DefaultTerminalLaunch(),
	}
}
func (value *CommandOpenToolTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandOpenToolTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"tool", "launch"}, false); err != nil {
		return err
	}
	*value = CommandOpenToolTerminalPayload(decoded)
	return nil
}
func (value CommandOpenToolTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandOpenToolTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListTerminalToolsPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandListTerminalToolsPayload() CommandListTerminalToolsPayload {
	return CommandListTerminalToolsPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandListTerminalToolsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListTerminalToolsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandListTerminalToolsPayload(decoded)
	return nil
}
func (value CommandListTerminalToolsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListTerminalToolsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCloseTerminalPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Terminal TerminalID `json:"terminal"`
}

func DefaultCommandCloseTerminalPayload() CommandCloseTerminalPayload {
	return CommandCloseTerminalPayload{
		Worktree: WorktreeID{},
		Terminal: TerminalID{},
	}
}
func (value *CommandCloseTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCloseTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "terminal"}, false); err != nil {
		return err
	}
	*value = CommandCloseTerminalPayload(decoded)
	return nil
}
func (value CommandCloseTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCloseTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListTerminalsPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandListTerminalsPayload() CommandListTerminalsPayload {
	return CommandListTerminalsPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandListTerminalsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListTerminalsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandListTerminalsPayload(decoded)
	return nil
}
func (value CommandListTerminalsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListTerminalsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInspectTerminalPayload struct {
	Terminal TerminalID `json:"terminal"`
}

func DefaultCommandInspectTerminalPayload() CommandInspectTerminalPayload {
	return CommandInspectTerminalPayload{
		Terminal: TerminalID{},
	}
}
func (value *CommandInspectTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInspectTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal"}, false); err != nil {
		return err
	}
	*value = CommandInspectTerminalPayload(decoded)
	return nil
}
func (value CommandInspectTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInspectTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandClaimTerminalPayload struct {
	Terminal         TerminalID `json:"terminal"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandClaimTerminalPayload() CommandClaimTerminalPayload {
	return CommandClaimTerminalPayload{
		Terminal:         TerminalID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandClaimTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandClaimTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandClaimTerminalPayload(decoded)
	return nil
}
func (value CommandClaimTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandClaimTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInputTerminalPayload struct {
	Terminal TerminalID      `json:"terminal"`
	Revision uint64          `json:"revision"`
	Inputs   []TerminalInput `json:"inputs"`
	Bytes    Bytes           `json:"bytes,omitempty"`
	Clear    bool            `json:"clear,omitempty"`
}

func DefaultCommandInputTerminalPayload() CommandInputTerminalPayload {
	return CommandInputTerminalPayload{
		Terminal: TerminalID{},
		Revision: 0,
		Inputs:   []TerminalInput{},
	}
}
func (value *CommandInputTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInputTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal", "revision", "inputs"}, false); err != nil {
		return err
	}
	*value = CommandInputTerminalPayload(decoded)
	return nil
}
func (value CommandInputTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInputTerminalPayload
	encoded := plain(value)
	encoded.Inputs = normalizeCollections(encoded.Inputs)
	return json.Marshal(encoded)
}

type CommandResizeTerminalPayload struct {
	Terminal TerminalID       `json:"terminal"`
	Revision uint64           `json:"revision"`
	Viewport TerminalViewport `json:"viewport"`
}

func DefaultCommandResizeTerminalPayload() CommandResizeTerminalPayload {
	return CommandResizeTerminalPayload{
		Terminal: TerminalID{},
		Revision: 0,
		Viewport: DefaultTerminalViewport(),
	}
}
func (value *CommandResizeTerminalPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResizeTerminalPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal", "revision", "viewport"}, false); err != nil {
		return err
	}
	*value = CommandResizeTerminalPayload(decoded)
	return nil
}
func (value CommandResizeTerminalPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResizeTerminalPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetTerminalAppearancePayload struct {
	Terminal   TerminalID         `json:"terminal"`
	Revision   uint64             `json:"revision"`
	Appearance TerminalAppearance `json:"appearance"`
}

func DefaultCommandSetTerminalAppearancePayload() CommandSetTerminalAppearancePayload {
	return CommandSetTerminalAppearancePayload{
		Terminal:   TerminalID{},
		Revision:   0,
		Appearance: DefaultTerminalAppearance(),
	}
}
func (value *CommandSetTerminalAppearancePayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetTerminalAppearancePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal", "revision", "appearance"}, false); err != nil {
		return err
	}
	*value = CommandSetTerminalAppearancePayload(decoded)
	return nil
}
func (value CommandSetTerminalAppearancePayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetTerminalAppearancePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadHostMetricsPayload struct {
	Detailed bool `json:"detailed"`
}

func DefaultCommandReadHostMetricsPayload() CommandReadHostMetricsPayload {
	return CommandReadHostMetricsPayload{
		Detailed: false,
	}
}
func (value *CommandReadHostMetricsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadHostMetricsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"detailed"}, false); err != nil {
		return err
	}
	*value = CommandReadHostMetricsPayload(decoded)
	return nil
}
func (value CommandReadHostMetricsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadHostMetricsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUseMediaPayload struct {
	Turn     TurnID      `json:"turn"`
	Session  SessionID   `json:"session"`
	Worktree WorktreeID  `json:"worktree"`
	Action   MediaAction `json:"action"`
}

func DefaultCommandUseMediaPayload() CommandUseMediaPayload {
	return CommandUseMediaPayload{
		Turn:     TurnID{},
		Session:  SessionID{},
		Worktree: WorktreeID{},
		Action:   MediaAction{},
	}
}
func (value *CommandUseMediaPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUseMediaPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "session", "worktree", "action"}, false); err != nil {
		return err
	}
	*value = CommandUseMediaPayload(decoded)
	return nil
}
func (value CommandUseMediaPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUseMediaPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRequestComputerPermissionPayload struct {
	Permission ComputerPermission `json:"permission"`
}

func DefaultCommandRequestComputerPermissionPayload() CommandRequestComputerPermissionPayload {
	return CommandRequestComputerPermissionPayload{
		Permission: ComputerPermission(""),
	}
}
func (value *CommandRequestComputerPermissionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRequestComputerPermissionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"permission"}, false); err != nil {
		return err
	}
	*value = CommandRequestComputerPermissionPayload(decoded)
	return nil
}
func (value CommandRequestComputerPermissionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRequestComputerPermissionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandStopHostProcessPayload struct {
	Pid           uint32 `json:"pid"`
	StartedAtSecs uint64 `json:"started_at_secs"`
	Force         bool   `json:"force"`
}

func DefaultCommandStopHostProcessPayload() CommandStopHostProcessPayload {
	return CommandStopHostProcessPayload{
		Pid:           0,
		StartedAtSecs: 0,
		Force:         false,
	}
}
func (value *CommandStopHostProcessPayload) UnmarshalJSON(data []byte) error {
	type plain CommandStopHostProcessPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"pid", "started_at_secs", "force"}, false); err != nil {
		return err
	}
	*value = CommandStopHostProcessPayload(decoded)
	return nil
}
func (value CommandStopHostProcessPayload) MarshalJSON() ([]byte, error) {
	type plain CommandStopHostProcessPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInspectRequestPayload struct {
	ID     RequestID `json:"id"`
	Digest string    `json:"digest"`
}

func DefaultCommandInspectRequestPayload() CommandInspectRequestPayload {
	return CommandInspectRequestPayload{
		ID:     RequestID{},
		Digest: "",
	}
}
func (value *CommandInspectRequestPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInspectRequestPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "digest"}, false); err != nil {
		return err
	}
	*value = CommandInspectRequestPayload(decoded)
	return nil
}
func (value CommandInspectRequestPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInspectRequestPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandInspectGitPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandInspectGitPayload() CommandInspectGitPayload {
	return CommandInspectGitPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandInspectGitPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInspectGitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandInspectGitPayload(decoded)
	return nil
}
func (value CommandInspectGitPayload) MarshalJSON() ([]byte, error) {
	type plain CommandInspectGitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitHeadPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandReadGitHeadPayload() CommandReadGitHeadPayload {
	return CommandReadGitHeadPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandReadGitHeadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitHeadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandReadGitHeadPayload(decoded)
	return nil
}
func (value CommandReadGitHeadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitHeadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListGitBranchesPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandListGitBranchesPayload() CommandListGitBranchesPayload {
	return CommandListGitBranchesPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandListGitBranchesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListGitBranchesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandListGitBranchesPayload(decoded)
	return nil
}
func (value CommandListGitBranchesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListGitBranchesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandResolveGitRevisionPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Revision string     `json:"revision"`
}

func DefaultCommandResolveGitRevisionPayload() CommandResolveGitRevisionPayload {
	return CommandResolveGitRevisionPayload{
		Worktree: WorktreeID{},
		Revision: "",
	}
}
func (value *CommandResolveGitRevisionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandResolveGitRevisionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "revision"}, false); err != nil {
		return err
	}
	*value = CommandResolveGitRevisionPayload(decoded)
	return nil
}
func (value CommandResolveGitRevisionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandResolveGitRevisionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListWorktreesPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandListWorktreesPayload() CommandListWorktreesPayload {
	return CommandListWorktreesPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandListWorktreesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListWorktreesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandListWorktreesPayload(decoded)
	return nil
}
func (value CommandListWorktreesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListWorktreesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRegisterWorktreePayload struct {
	Project ProjectID `json:"project"`
	Path    string    `json:"path"`
}

func DefaultCommandRegisterWorktreePayload() CommandRegisterWorktreePayload {
	return CommandRegisterWorktreePayload{
		Project: ProjectID{},
		Path:    "",
	}
}
func (value *CommandRegisterWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRegisterWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"project", "path"}, false); err != nil {
		return err
	}
	*value = CommandRegisterWorktreePayload(decoded)
	return nil
}
func (value CommandRegisterWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRegisterWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateWorktreePayload struct {
	Project ProjectID `json:"project"`
	Path    string    `json:"path"`
	Branch  string    `json:"branch"`
	Commit  string    `json:"commit"`
}

func DefaultCommandCreateWorktreePayload() CommandCreateWorktreePayload {
	return CommandCreateWorktreePayload{
		Project: ProjectID{},
		Path:    "",
		Branch:  "",
		Commit:  "",
	}
}
func (value *CommandCreateWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"project", "path", "branch", "commit"}, false); err != nil {
		return err
	}
	*value = CommandCreateWorktreePayload(decoded)
	return nil
}
func (value CommandCreateWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateManagedWorktreePayload struct {
	Project        ProjectID  `json:"project"`
	Source         WorktreeID `json:"source"`
	Branch         string     `json:"branch"`
	ExpectedHead   string     `json:"expected_head"`
	ExpectedIndex  string     `json:"expected_index"`
	IncludeChanges bool       `json:"include_changes"`
	Base           *string    `json:"base,omitempty"`
}

func DefaultCommandCreateManagedWorktreePayload() CommandCreateManagedWorktreePayload {
	return CommandCreateManagedWorktreePayload{
		Project:        ProjectID{},
		Source:         WorktreeID{},
		Branch:         "",
		ExpectedHead:   "",
		ExpectedIndex:  "",
		IncludeChanges: false,
		Base:           nil,
	}
}
func (value *CommandCreateManagedWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateManagedWorktreePayload
	decoded := plain{}
	decoded.Base = nil
	if err := DecodeObject(data, &decoded, []string{"project", "source", "branch", "expected_head", "expected_index", "include_changes"}, false); err != nil {
		return err
	}
	*value = CommandCreateManagedWorktreePayload(decoded)
	return nil
}
func (value CommandCreateManagedWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateManagedWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveWorktreePayload struct {
	Worktree       WorktreeID `json:"worktree"`
	ExpectedHead   string     `json:"expected_head"`
	ExpectedBranch string     `json:"expected_branch"`
}

func DefaultCommandRemoveWorktreePayload() CommandRemoveWorktreePayload {
	return CommandRemoveWorktreePayload{
		Worktree:       WorktreeID{},
		ExpectedHead:   "",
		ExpectedBranch: "",
	}
}
func (value *CommandRemoveWorktreePayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveWorktreePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "expected_head", "expected_branch"}, false); err != nil {
		return err
	}
	*value = CommandRemoveWorktreePayload(decoded)
	return nil
}
func (value CommandRemoveWorktreePayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveWorktreePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateGitBranchPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Name     string     `json:"name"`
	Commit   string     `json:"commit"`
}

func DefaultCommandCreateGitBranchPayload() CommandCreateGitBranchPayload {
	return CommandCreateGitBranchPayload{
		Worktree: WorktreeID{},
		Name:     "",
		Commit:   "",
	}
}
func (value *CommandCreateGitBranchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateGitBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "commit"}, false); err != nil {
		return err
	}
	*value = CommandCreateGitBranchPayload(decoded)
	return nil
}
func (value CommandCreateGitBranchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateGitBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRenameGitBranchPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Name     string     `json:"name"`
	NewName  string     `json:"new_name"`
	Commit   string     `json:"commit"`
}

func DefaultCommandRenameGitBranchPayload() CommandRenameGitBranchPayload {
	return CommandRenameGitBranchPayload{
		Worktree: WorktreeID{},
		Name:     "",
		NewName:  "",
		Commit:   "",
	}
}
func (value *CommandRenameGitBranchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRenameGitBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "new_name", "commit"}, false); err != nil {
		return err
	}
	*value = CommandRenameGitBranchPayload(decoded)
	return nil
}
func (value CommandRenameGitBranchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRenameGitBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDeleteGitBranchPayload struct {
	Worktree       WorktreeID `json:"worktree"`
	Name           string     `json:"name"`
	Commit         string     `json:"commit"`
	ExpectedHead   string     `json:"expected_head"`
	ExpectedBranch *string    `json:"expected_branch"`
}

func DefaultCommandDeleteGitBranchPayload() CommandDeleteGitBranchPayload {
	return CommandDeleteGitBranchPayload{
		Worktree:       WorktreeID{},
		Name:           "",
		Commit:         "",
		ExpectedHead:   "",
		ExpectedBranch: nil,
	}
}
func (value *CommandDeleteGitBranchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDeleteGitBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "commit", "expected_head"}, false); err != nil {
		return err
	}
	*value = CommandDeleteGitBranchPayload(decoded)
	return nil
}
func (value CommandDeleteGitBranchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDeleteGitBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSwitchGitBranchPayload struct {
	Worktree       WorktreeID `json:"worktree"`
	Name           string     `json:"name"`
	Commit         string     `json:"commit"`
	ExpectedHead   *string    `json:"expected_head"`
	ExpectedBranch *string    `json:"expected_branch"`
	ExpectedIndex  string     `json:"expected_index"`
}

func DefaultCommandSwitchGitBranchPayload() CommandSwitchGitBranchPayload {
	return CommandSwitchGitBranchPayload{
		Worktree:       WorktreeID{},
		Name:           "",
		Commit:         "",
		ExpectedHead:   nil,
		ExpectedBranch: nil,
		ExpectedIndex:  "",
	}
}
func (value *CommandSwitchGitBranchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSwitchGitBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "commit", "expected_index"}, false); err != nil {
		return err
	}
	*value = CommandSwitchGitBranchPayload(decoded)
	return nil
}
func (value CommandSwitchGitBranchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSwitchGitBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMergeGitBranchPayload struct {
	Worktree       WorktreeID `json:"worktree"`
	Name           string     `json:"name"`
	Commit         string     `json:"commit"`
	ExpectedHead   string     `json:"expected_head"`
	ExpectedBranch string     `json:"expected_branch"`
	ExpectedIndex  string     `json:"expected_index"`
}

func DefaultCommandMergeGitBranchPayload() CommandMergeGitBranchPayload {
	return CommandMergeGitBranchPayload{
		Worktree:       WorktreeID{},
		Name:           "",
		Commit:         "",
		ExpectedHead:   "",
		ExpectedBranch: "",
		ExpectedIndex:  "",
	}
}
func (value *CommandMergeGitBranchPayload) UnmarshalJSON(data []byte) error {
	type plain CommandMergeGitBranchPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "commit", "expected_head", "expected_branch", "expected_index"}, false); err != nil {
		return err
	}
	*value = CommandMergeGitBranchPayload(decoded)
	return nil
}
func (value CommandMergeGitBranchPayload) MarshalJSON() ([]byte, error) {
	type plain CommandMergeGitBranchPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitStashPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Commit   string     `json:"commit"`
}

func DefaultCommandReadGitStashPayload() CommandReadGitStashPayload {
	return CommandReadGitStashPayload{
		Worktree: WorktreeID{},
		Commit:   "",
	}
}
func (value *CommandReadGitStashPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitStashPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "commit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitStashPayload(decoded)
	return nil
}
func (value CommandReadGitStashPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitStashPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitOutputPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandReadGitOutputPayload() CommandReadGitOutputPayload {
	return CommandReadGitOutputPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandReadGitOutputPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitOutputPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandReadGitOutputPayload(decoded)
	return nil
}
func (value CommandReadGitOutputPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitOutputPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListGitRemoteTagsPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Remote   string     `json:"remote"`
}

func DefaultCommandListGitRemoteTagsPayload() CommandListGitRemoteTagsPayload {
	return CommandListGitRemoteTagsPayload{
		Worktree: WorktreeID{},
		Remote:   "",
	}
}
func (value *CommandListGitRemoteTagsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListGitRemoteTagsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "remote"}, false); err != nil {
		return err
	}
	*value = CommandListGitRemoteTagsPayload(decoded)
	return nil
}
func (value CommandListGitRemoteTagsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListGitRemoteTagsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitLogPayload struct {
	Worktree WorktreeID    `json:"worktree"`
	Limit    uint64        `json:"limit"`
	Cursor   *GitLogCursor `json:"cursor"`
	Revision *string       `json:"revision"`
	Path     *string       `json:"path"`
}

func DefaultCommandReadGitLogPayload() CommandReadGitLogPayload {
	return CommandReadGitLogPayload{
		Worktree: WorktreeID{},
		Limit:    0,
		Cursor:   nil,
		Revision: nil,
		Path:     nil,
	}
}
func (value *CommandReadGitLogPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitLogPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitLogPayload(decoded)
	return nil
}
func (value CommandReadGitLogPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitLogPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitCommitPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Commit   string     `json:"commit"`
}

func DefaultCommandReadGitCommitPayload() CommandReadGitCommitPayload {
	return CommandReadGitCommitPayload{
		Worktree: WorktreeID{},
		Commit:   "",
	}
}
func (value *CommandReadGitCommitPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitCommitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "commit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitCommitPayload(decoded)
	return nil
}
func (value CommandReadGitCommitPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitCommitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListGitChangesPayload struct {
	Worktree WorktreeID        `json:"worktree"`
	Target   *GitChangeTarget  `json:"target"`
	Cursor   *GitChangesCursor `json:"cursor"`
	Limit    uint64            `json:"limit"`
}

func DefaultCommandListGitChangesPayload() CommandListGitChangesPayload {
	return CommandListGitChangesPayload{
		Worktree: WorktreeID{},
		Target:   nil,
		Cursor:   nil,
		Limit:    0,
	}
}
func (value *CommandListGitChangesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListGitChangesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "limit"}, false); err != nil {
		return err
	}
	*value = CommandListGitChangesPayload(decoded)
	return nil
}
func (value CommandListGitChangesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListGitChangesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitChangePayload struct {
	Worktree                 WorktreeID        `json:"worktree"`
	Snapshot                 GitChangeSnapshot `json:"snapshot"`
	Path                     string            `json:"path"`
	ExpectedMetadataRevision string            `json:"expected_metadata_revision"`
	ExpectedRevision         *string           `json:"expected_revision"`
	Offset                   uint64            `json:"offset"`
	Limit                    uint64            `json:"limit"`
}

func DefaultCommandReadGitChangePayload() CommandReadGitChangePayload {
	return CommandReadGitChangePayload{
		Worktree:                 WorktreeID{},
		Snapshot:                 DefaultGitChangeSnapshot(),
		Path:                     "",
		ExpectedMetadataRevision: "",
		ExpectedRevision:         nil,
		Offset:                   0,
		Limit:                    0,
	}
}
func (value *CommandReadGitChangePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitChangePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "snapshot", "path", "expected_metadata_revision", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitChangePayload(decoded)
	return nil
}
func (value CommandReadGitChangePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitChangePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitContextPayload struct {
	Worktree                 WorktreeID        `json:"worktree"`
	Snapshot                 GitChangeSnapshot `json:"snapshot"`
	Path                     string            `json:"path"`
	ExpectedMetadataRevision string            `json:"expected_metadata_revision"`
	ExpectedRevision         string            `json:"expected_revision"`
	Side                     GitContextSide    `json:"side"`
	Offset                   uint64            `json:"offset"`
	Limit                    uint64            `json:"limit"`
}

func DefaultCommandReadGitContextPayload() CommandReadGitContextPayload {
	return CommandReadGitContextPayload{
		Worktree:                 WorktreeID{},
		Snapshot:                 DefaultGitChangeSnapshot(),
		Path:                     "",
		ExpectedMetadataRevision: "",
		ExpectedRevision:         "",
		Side:                     GitContextSide(""),
		Offset:                   0,
		Limit:                    0,
	}
}
func (value *CommandReadGitContextPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitContextPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "snapshot", "path", "expected_metadata_revision", "expected_revision", "side", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitContextPayload(decoded)
	return nil
}
func (value CommandReadGitContextPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitContextPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitShowPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Revision string     `json:"revision"`
	Path     *string    `json:"path"`
	Offset   uint64     `json:"offset"`
	Limit    uint64     `json:"limit"`
}

func DefaultCommandReadGitShowPayload() CommandReadGitShowPayload {
	return CommandReadGitShowPayload{
		Worktree: WorktreeID{},
		Revision: "",
		Path:     nil,
		Offset:   0,
		Limit:    0,
	}
}
func (value *CommandReadGitShowPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitShowPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "revision", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitShowPayload(decoded)
	return nil
}
func (value CommandReadGitShowPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitShowPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadGitBlamePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Revision *string    `json:"revision"`
	Path     string     `json:"path"`
	Offset   uint64     `json:"offset"`
	Limit    uint64     `json:"limit"`
}

func DefaultCommandReadGitBlamePayload() CommandReadGitBlamePayload {
	return CommandReadGitBlamePayload{
		Worktree: WorktreeID{},
		Revision: nil,
		Path:     "",
		Offset:   0,
		Limit:    0,
	}
}
func (value *CommandReadGitBlamePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitBlamePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadGitBlamePayload(decoded)
	return nil
}
func (value CommandReadGitBlamePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitBlamePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSearchFilesPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Options  FileSearch `json:"options"`
}

func DefaultCommandSearchFilesPayload() CommandSearchFilesPayload {
	return CommandSearchFilesPayload{
		Worktree: WorktreeID{},
		Options:  DefaultFileSearch(),
	}
}
func (value *CommandSearchFilesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSearchFilesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "options"}, false); err != nil {
		return err
	}
	*value = CommandSearchFilesPayload(decoded)
	return nil
}
func (value CommandSearchFilesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSearchFilesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRunGitActionPayload struct {
	Worktree       WorktreeID `json:"worktree"`
	Action         GitAction  `json:"action"`
	ExpectedIndex  string     `json:"expected_index"`
	ExpectedHead   *string    `json:"expected_head"`
	ExpectedBranch *string    `json:"expected_branch"`
}

func DefaultCommandRunGitActionPayload() CommandRunGitActionPayload {
	return CommandRunGitActionPayload{
		Worktree:       WorktreeID{},
		Action:         GitAction{},
		ExpectedIndex:  "",
		ExpectedHead:   nil,
		ExpectedBranch: nil,
	}
}
func (value *CommandRunGitActionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRunGitActionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "action", "expected_index"}, false); err != nil {
		return err
	}
	*value = CommandRunGitActionPayload(decoded)
	return nil
}
func (value CommandRunGitActionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRunGitActionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateGitCommitPayload struct {
	Worktree       WorktreeID       `json:"worktree"`
	Message        string           `json:"message"`
	Amend          bool             `json:"amend"`
	Options        GitCommitOptions `json:"options"`
	ExpectedIndex  string           `json:"expected_index"`
	ExpectedHead   *string          `json:"expected_head"`
	ExpectedBranch *string          `json:"expected_branch"`
}

func DefaultCommandCreateGitCommitPayload() CommandCreateGitCommitPayload {
	return CommandCreateGitCommitPayload{
		Worktree:       WorktreeID{},
		Message:        "",
		Amend:          false,
		Options:        DefaultGitCommitOptions(),
		ExpectedIndex:  "",
		ExpectedHead:   nil,
		ExpectedBranch: nil,
	}
}
func (value *CommandCreateGitCommitPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateGitCommitPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "message", "amend", "options", "expected_index"}, false); err != nil {
		return err
	}
	*value = CommandCreateGitCommitPayload(decoded)
	return nil
}
func (value CommandCreateGitCommitPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateGitCommitPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUpdateGitIndexPayload struct {
	Worktree      WorktreeID     `json:"worktree"`
	Paths         []string       `json:"paths"`
	Operation     GitIndexChange `json:"operation"`
	ExpectedIndex string         `json:"expected_index"`
	ExpectedHead  *string        `json:"expected_head"`
}

func DefaultCommandUpdateGitIndexPayload() CommandUpdateGitIndexPayload {
	return CommandUpdateGitIndexPayload{
		Worktree:      WorktreeID{},
		Paths:         []string{},
		Operation:     GitIndexChange(""),
		ExpectedIndex: "",
		ExpectedHead:  nil,
	}
}
func (value *CommandUpdateGitIndexPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUpdateGitIndexPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "paths", "operation", "expected_index"}, false); err != nil {
		return err
	}
	*value = CommandUpdateGitIndexPayload(decoded)
	return nil
}
func (value CommandUpdateGitIndexPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUpdateGitIndexPayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type CommandReadGitDiffPayload struct {
	Worktree WorktreeID   `json:"worktree"`
	Path     string       `json:"path"`
	Scope    GitDiffScope `json:"scope"`
}

func DefaultCommandReadGitDiffPayload() CommandReadGitDiffPayload {
	return CommandReadGitDiffPayload{
		Worktree: WorktreeID{},
		Path:     "",
		Scope:    GitDiffScope(""),
	}
}
func (value *CommandReadGitDiffPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadGitDiffPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "scope"}, false); err != nil {
		return err
	}
	*value = CommandReadGitDiffPayload(decoded)
	return nil
}
func (value CommandReadGitDiffPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadGitDiffPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandListDirectoryPayload struct {
	Worktree WorktreeID       `json:"worktree"`
	Path     string           `json:"path"`
	After    *DirectoryCursor `json:"after"`
}

func DefaultCommandListDirectoryPayload() CommandListDirectoryPayload {
	return CommandListDirectoryPayload{
		Worktree: WorktreeID{},
		Path:     "",
		After:    nil,
	}
}
func (value *CommandListDirectoryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandListDirectoryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandListDirectoryPayload(decoded)
	return nil
}
func (value CommandListDirectoryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandListDirectoryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadFilePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandReadFilePayload() CommandReadFilePayload {
	return CommandReadFilePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandReadFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandReadFilePayload(decoded)
	return nil
}
func (value CommandReadFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSearchCodePayload struct {
	Worktree WorktreeID      `json:"worktree"`
	Query    CodeIndexSearch `json:"query"`
}

func DefaultCommandSearchCodePayload() CommandSearchCodePayload {
	return CommandSearchCodePayload{
		Worktree: WorktreeID{},
		Query:    DefaultCodeIndexSearch(),
	}
}
func (value *CommandSearchCodePayload) UnmarshalJSON(data []byte) error {
	type plain CommandSearchCodePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "query"}, false); err != nil {
		return err
	}
	*value = CommandSearchCodePayload(decoded)
	return nil
}
func (value CommandSearchCodePayload) MarshalJSON() ([]byte, error) {
	type plain CommandSearchCodePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandOutlineCodePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandOutlineCodePayload() CommandOutlineCodePayload {
	return CommandOutlineCodePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandOutlineCodePayload) UnmarshalJSON(data []byte) error {
	type plain CommandOutlineCodePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandOutlineCodePayload(decoded)
	return nil
}
func (value CommandOutlineCodePayload) MarshalJSON() ([]byte, error) {
	type plain CommandOutlineCodePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandGraphCodePayload struct {
	Worktree WorktreeID     `json:"worktree"`
	Query    CodeIndexGraph `json:"query"`
}

func DefaultCommandGraphCodePayload() CommandGraphCodePayload {
	return CommandGraphCodePayload{
		Worktree: WorktreeID{},
		Query:    DefaultCodeIndexGraph(),
	}
}
func (value *CommandGraphCodePayload) UnmarshalJSON(data []byte) error {
	type plain CommandGraphCodePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "query"}, false); err != nil {
		return err
	}
	*value = CommandGraphCodePayload(decoded)
	return nil
}
func (value CommandGraphCodePayload) MarshalJSON() ([]byte, error) {
	type plain CommandGraphCodePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelCodeIndexQueryPayload struct {
	Request RequestID `json:"request"`
}

func DefaultCommandCancelCodeIndexQueryPayload() CommandCancelCodeIndexQueryPayload {
	return CommandCancelCodeIndexQueryPayload{
		Request: RequestID{},
	}
}
func (value *CommandCancelCodeIndexQueryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelCodeIndexQueryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = CommandCancelCodeIndexQueryPayload(decoded)
	return nil
}
func (value CommandCancelCodeIndexQueryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelCodeIndexQueryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadHooksPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandReadHooksPayload() CommandReadHooksPayload {
	return CommandReadHooksPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandReadHooksPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadHooksPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandReadHooksPayload(decoded)
	return nil
}
func (value CommandReadHooksPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadHooksPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSaveHooksPayload struct {
	Worktree         WorktreeID    `json:"worktree"`
	Settings         HooksSettings `json:"settings"`
	ExpectedRevision uint64        `json:"expected_revision"`
}

func DefaultCommandSaveHooksPayload() CommandSaveHooksPayload {
	return CommandSaveHooksPayload{
		Worktree:         WorktreeID{},
		Settings:         DefaultHooksSettings(),
		ExpectedRevision: 0,
	}
}
func (value *CommandSaveHooksPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSaveHooksPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "settings", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSaveHooksPayload(decoded)
	return nil
}
func (value CommandSaveHooksPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSaveHooksPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSaveCodeIndexSettingsPayload struct {
	Settings         CodeIndexSettings `json:"settings"`
	ExpectedRevision uint64            `json:"expected_revision"`
}

func DefaultCommandSaveCodeIndexSettingsPayload() CommandSaveCodeIndexSettingsPayload {
	return CommandSaveCodeIndexSettingsPayload{
		Settings:         DefaultCodeIndexSettings(),
		ExpectedRevision: 0,
	}
}
func (value *CommandSaveCodeIndexSettingsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSaveCodeIndexSettingsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"settings", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSaveCodeIndexSettingsPayload(decoded)
	return nil
}
func (value CommandSaveCodeIndexSettingsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSaveCodeIndexSettingsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadFileLinesPayload struct {
	Worktree         WorktreeID `json:"worktree"`
	Path             string     `json:"path"`
	Offset           uint64     `json:"offset"`
	Limit            uint64     `json:"limit"`
	ExpectedRevision *string    `json:"expected_revision"`
}

func DefaultCommandReadFileLinesPayload() CommandReadFileLinesPayload {
	return CommandReadFileLinesPayload{
		Worktree:         WorktreeID{},
		Path:             "",
		Offset:           0,
		Limit:            0,
		ExpectedRevision: nil,
	}
}
func (value *CommandReadFileLinesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadFileLinesPayload
	decoded := plain{}
	decoded.ExpectedRevision = nil
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "offset", "limit"}, false); err != nil {
		return err
	}
	*value = CommandReadFileLinesPayload(decoded)
	return nil
}
func (value CommandReadFileLinesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadFileLinesPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadImagePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandReadImagePayload() CommandReadImagePayload {
	return CommandReadImagePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandReadImagePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadImagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandReadImagePayload(decoded)
	return nil
}
func (value CommandReadImagePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadImagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandFindFilesPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Globs    []string   `json:"globs"`
	Path     string     `json:"path"`
	Limit    uint64     `json:"limit"`
	Offset   uint64     `json:"offset"`
}

func DefaultCommandFindFilesPayload() CommandFindFilesPayload {
	return CommandFindFilesPayload{
		Worktree: WorktreeID{},
		Globs:    []string{},
		Path:     "",
		Limit:    0,
		Offset:   0,
	}
}
func (value *CommandFindFilesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandFindFilesPayload
	decoded := plain{}
	decoded.Path = ""
	decoded.Limit = 500
	decoded.Offset = 0
	if err := DecodeObject(data, &decoded, []string{"worktree", "globs"}, false); err != nil {
		return err
	}
	*value = CommandFindFilesPayload(decoded)
	return nil
}
func (value CommandFindFilesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandFindFilesPayload
	encoded := plain(value)
	encoded.Globs = normalizeCollections(encoded.Globs)
	return json.Marshal(encoded)
}

type CommandReadOfficePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Options  OfficeRead `json:"options"`
}

func DefaultCommandReadOfficePayload() CommandReadOfficePayload {
	return CommandReadOfficePayload{
		Worktree: WorktreeID{},
		Options:  DefaultOfficeRead(),
	}
}
func (value *CommandReadOfficePayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadOfficePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "options"}, false); err != nil {
		return err
	}
	*value = CommandReadOfficePayload(decoded)
	return nil
}
func (value CommandReadOfficePayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadOfficePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadNotebookPayload struct {
	Worktree WorktreeID   `json:"worktree"`
	Options  NotebookRead `json:"options"`
}

func DefaultCommandReadNotebookPayload() CommandReadNotebookPayload {
	return CommandReadNotebookPayload{
		Worktree: WorktreeID{},
		Options:  DefaultNotebookRead(),
	}
}
func (value *CommandReadNotebookPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadNotebookPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "options"}, false); err != nil {
		return err
	}
	*value = CommandReadNotebookPayload(decoded)
	return nil
}
func (value CommandReadNotebookPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadNotebookPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandEditNotebookPayload struct {
	Worktree WorktreeID   `json:"worktree"`
	Edit     NotebookEdit `json:"edit"`
}

func DefaultCommandEditNotebookPayload() CommandEditNotebookPayload {
	return CommandEditNotebookPayload{
		Worktree: WorktreeID{},
		Edit:     DefaultNotebookEdit(),
	}
}
func (value *CommandEditNotebookPayload) UnmarshalJSON(data []byte) error {
	type plain CommandEditNotebookPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "edit"}, false); err != nil {
		return err
	}
	*value = CommandEditNotebookPayload(decoded)
	return nil
}
func (value CommandEditNotebookPayload) MarshalJSON() ([]byte, error) {
	type plain CommandEditNotebookPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDownloadFilePayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandDownloadFilePayload() CommandDownloadFilePayload {
	return CommandDownloadFilePayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandDownloadFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandDownloadFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandDownloadFilePayload(decoded)
	return nil
}
func (value CommandDownloadFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandDownloadFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandFinishAttachmentUploadPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Stream   StreamID   `json:"stream"`
}

func DefaultCommandFinishAttachmentUploadPayload() CommandFinishAttachmentUploadPayload {
	return CommandFinishAttachmentUploadPayload{
		Worktree: WorktreeID{},
		Stream:   StreamID{},
	}
}
func (value *CommandFinishAttachmentUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandFinishAttachmentUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "stream"}, false); err != nil {
		return err
	}
	*value = CommandFinishAttachmentUploadPayload(decoded)
	return nil
}
func (value CommandFinishAttachmentUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandFinishAttachmentUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadAttachmentPayload struct {
	Worktree   WorktreeID   `json:"worktree"`
	Attachment AttachmentID `json:"attachment"`
}

func DefaultCommandReadAttachmentPayload() CommandReadAttachmentPayload {
	return CommandReadAttachmentPayload{
		Worktree:   WorktreeID{},
		Attachment: AttachmentID{},
	}
}
func (value *CommandReadAttachmentPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadAttachmentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "attachment"}, false); err != nil {
		return err
	}
	*value = CommandReadAttachmentPayload(decoded)
	return nil
}
func (value CommandReadAttachmentPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadAttachmentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDownloadAttachmentPayload struct {
	Worktree   WorktreeID   `json:"worktree"`
	Attachment AttachmentID `json:"attachment"`
}

func DefaultCommandDownloadAttachmentPayload() CommandDownloadAttachmentPayload {
	return CommandDownloadAttachmentPayload{
		Worktree:   WorktreeID{},
		Attachment: AttachmentID{},
	}
}
func (value *CommandDownloadAttachmentPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDownloadAttachmentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "attachment"}, false); err != nil {
		return err
	}
	*value = CommandDownloadAttachmentPayload(decoded)
	return nil
}
func (value CommandDownloadAttachmentPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDownloadAttachmentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDownloadImagePayload struct {
	Session SessionID         `json:"session"`
	Image   ConversationImage `json:"image"`
}

func DefaultCommandDownloadImagePayload() CommandDownloadImagePayload {
	return CommandDownloadImagePayload{
		Session: SessionID{},
		Image:   DefaultConversationImage(),
	}
}
func (value *CommandDownloadImagePayload) UnmarshalJSON(data []byte) error {
	type plain CommandDownloadImagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "image"}, false); err != nil {
		return err
	}
	*value = CommandDownloadImagePayload(decoded)
	return nil
}
func (value CommandDownloadImagePayload) MarshalJSON() ([]byte, error) {
	type plain CommandDownloadImagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandDiscardAttachmentPayload struct {
	Worktree   WorktreeID   `json:"worktree"`
	Attachment AttachmentID `json:"attachment"`
}

func DefaultCommandDiscardAttachmentPayload() CommandDiscardAttachmentPayload {
	return CommandDiscardAttachmentPayload{
		Worktree:   WorktreeID{},
		Attachment: AttachmentID{},
	}
}
func (value *CommandDiscardAttachmentPayload) UnmarshalJSON(data []byte) error {
	type plain CommandDiscardAttachmentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "attachment"}, false); err != nil {
		return err
	}
	*value = CommandDiscardAttachmentPayload(decoded)
	return nil
}
func (value CommandDiscardAttachmentPayload) MarshalJSON() ([]byte, error) {
	type plain CommandDiscardAttachmentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandFinishFileUploadPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
	Stream   StreamID   `json:"stream"`
}

func DefaultCommandFinishFileUploadPayload() CommandFinishFileUploadPayload {
	return CommandFinishFileUploadPayload{
		Worktree: WorktreeID{},
		Path:     "",
		Stream:   StreamID{},
	}
}
func (value *CommandFinishFileUploadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandFinishFileUploadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "stream"}, false); err != nil {
		return err
	}
	*value = CommandFinishFileUploadPayload(decoded)
	return nil
}
func (value CommandFinishFileUploadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandFinishFileUploadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCancelFileTransferPayload struct {
	Stream StreamID `json:"stream"`
}

func DefaultCommandCancelFileTransferPayload() CommandCancelFileTransferPayload {
	return CommandCancelFileTransferPayload{
		Stream: StreamID{},
	}
}
func (value *CommandCancelFileTransferPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCancelFileTransferPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream"}, false); err != nil {
		return err
	}
	*value = CommandCancelFileTransferPayload(decoded)
	return nil
}
func (value CommandCancelFileTransferPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCancelFileTransferPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandWriteFilePayload struct {
	Worktree         WorktreeID `json:"worktree"`
	Path             string     `json:"path"`
	Text             string     `json:"text"`
	ExpectedRevision *string    `json:"expected_revision"`
}

func DefaultCommandWriteFilePayload() CommandWriteFilePayload {
	return CommandWriteFilePayload{
		Worktree:         WorktreeID{},
		Path:             "",
		Text:             "",
		ExpectedRevision: nil,
	}
}
func (value *CommandWriteFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandWriteFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "text"}, false); err != nil {
		return err
	}
	*value = CommandWriteFilePayload(decoded)
	return nil
}
func (value CommandWriteFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandWriteFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandEditFilePayload struct {
	Worktree         WorktreeID  `json:"worktree"`
	Path             string      `json:"path"`
	OldText          *string     `json:"old_text,omitempty"`
	NewText          *string     `json:"new_text,omitempty"`
	ReplaceAll       bool        `json:"replace_all,omitempty"`
	Edits            *[]FileEdit `json:"edits,omitempty"`
	ExpectedRevision string      `json:"expected_revision"`
}

func DefaultCommandEditFilePayload() CommandEditFilePayload {
	return CommandEditFilePayload{
		Worktree:         WorktreeID{},
		Path:             "",
		OldText:          nil,
		NewText:          nil,
		ReplaceAll:       false,
		Edits:            nil,
		ExpectedRevision: "",
	}
}
func (value *CommandEditFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandEditFilePayload
	decoded := plain{}
	decoded.OldText = nil
	decoded.NewText = nil
	decoded.ReplaceAll = false
	decoded.Edits = nil
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandEditFilePayload(decoded)
	return nil
}
func (value CommandEditFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandEditFilePayload
	encoded := plain(value)
	encoded.Edits = normalizeCollections(encoded.Edits)
	return json.Marshal(encoded)
}

type CommandDeleteFilePayload struct {
	Worktree         WorktreeID `json:"worktree"`
	Path             string     `json:"path"`
	ExpectedRevision string     `json:"expected_revision"`
}

func DefaultCommandDeleteFilePayload() CommandDeleteFilePayload {
	return CommandDeleteFilePayload{
		Worktree:         WorktreeID{},
		Path:             "",
		ExpectedRevision: "",
	}
}
func (value *CommandDeleteFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandDeleteFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandDeleteFilePayload(decoded)
	return nil
}
func (value CommandDeleteFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandDeleteFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMoveFilePayload struct {
	Worktree         WorktreeID `json:"worktree"`
	From             string     `json:"from"`
	To               string     `json:"to"`
	ExpectedRevision string     `json:"expected_revision"`
}

func DefaultCommandMoveFilePayload() CommandMoveFilePayload {
	return CommandMoveFilePayload{
		Worktree:         WorktreeID{},
		From:             "",
		To:               "",
		ExpectedRevision: "",
	}
}
func (value *CommandMoveFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandMoveFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "from", "to", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandMoveFilePayload(decoded)
	return nil
}
func (value CommandMoveFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandMoveFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandApplyChangesPayload struct {
	Worktree WorktreeID   `json:"worktree"`
	Changes  []FileChange `json:"changes"`
}

func DefaultCommandApplyChangesPayload() CommandApplyChangesPayload {
	return CommandApplyChangesPayload{
		Worktree: WorktreeID{},
		Changes:  []FileChange{},
	}
}
func (value *CommandApplyChangesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandApplyChangesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "changes"}, false); err != nil {
		return err
	}
	*value = CommandApplyChangesPayload(decoded)
	return nil
}
func (value CommandApplyChangesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandApplyChangesPayload
	encoded := plain(value)
	encoded.Changes = normalizeCollections(encoded.Changes)
	return json.Marshal(encoded)
}

type CommandCreateDirectoryPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandCreateDirectoryPayload() CommandCreateDirectoryPayload {
	return CommandCreateDirectoryPayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandCreateDirectoryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateDirectoryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandCreateDirectoryPayload(decoded)
	return nil
}
func (value CommandCreateDirectoryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateDirectoryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRenameEntryPayload struct {
	Worktree WorktreeID `json:"worktree"`
	From     string     `json:"from"`
	To       string     `json:"to"`
}

func DefaultCommandRenameEntryPayload() CommandRenameEntryPayload {
	return CommandRenameEntryPayload{
		Worktree: WorktreeID{},
		From:     "",
		To:       "",
	}
}
func (value *CommandRenameEntryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRenameEntryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "from", "to"}, false); err != nil {
		return err
	}
	*value = CommandRenameEntryPayload(decoded)
	return nil
}
func (value CommandRenameEntryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRenameEntryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCopyEntryPayload struct {
	Worktree WorktreeID `json:"worktree"`
	From     string     `json:"from"`
	To       string     `json:"to"`
}

func DefaultCommandCopyEntryPayload() CommandCopyEntryPayload {
	return CommandCopyEntryPayload{
		Worktree: WorktreeID{},
		From:     "",
		To:       "",
	}
}
func (value *CommandCopyEntryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCopyEntryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "from", "to"}, false); err != nil {
		return err
	}
	*value = CommandCopyEntryPayload(decoded)
	return nil
}
func (value CommandCopyEntryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCopyEntryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCopyEntryToPayload struct {
	Source   WorktreeID `json:"source"`
	Worktree WorktreeID `json:"worktree"`
	From     string     `json:"from"`
	To       string     `json:"to"`
}

func DefaultCommandCopyEntryToPayload() CommandCopyEntryToPayload {
	return CommandCopyEntryToPayload{
		Source:   WorktreeID{},
		Worktree: WorktreeID{},
		From:     "",
		To:       "",
	}
}
func (value *CommandCopyEntryToPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCopyEntryToPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "worktree", "from", "to"}, false); err != nil {
		return err
	}
	*value = CommandCopyEntryToPayload(decoded)
	return nil
}
func (value CommandCopyEntryToPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCopyEntryToPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMoveEntryToPayload struct {
	Source   WorktreeID `json:"source"`
	Worktree WorktreeID `json:"worktree"`
	From     string     `json:"from"`
	To       string     `json:"to"`
}

func DefaultCommandMoveEntryToPayload() CommandMoveEntryToPayload {
	return CommandMoveEntryToPayload{
		Source:   WorktreeID{},
		Worktree: WorktreeID{},
		From:     "",
		To:       "",
	}
}
func (value *CommandMoveEntryToPayload) UnmarshalJSON(data []byte) error {
	type plain CommandMoveEntryToPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"source", "worktree", "from", "to"}, false); err != nil {
		return err
	}
	*value = CommandMoveEntryToPayload(decoded)
	return nil
}
func (value CommandMoveEntryToPayload) MarshalJSON() ([]byte, error) {
	type plain CommandMoveEntryToPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandTrashEntryPayload struct {
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
}

func DefaultCommandTrashEntryPayload() CommandTrashEntryPayload {
	return CommandTrashEntryPayload{
		Worktree: WorktreeID{},
		Path:     "",
	}
}
func (value *CommandTrashEntryPayload) UnmarshalJSON(data []byte) error {
	type plain CommandTrashEntryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path"}, false); err != nil {
		return err
	}
	*value = CommandTrashEntryPayload(decoded)
	return nil
}
func (value CommandTrashEntryPayload) MarshalJSON() ([]byte, error) {
	type plain CommandTrashEntryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandTrashFilePayload struct {
	Worktree         WorktreeID `json:"worktree"`
	Path             string     `json:"path"`
	ExpectedRevision string     `json:"expected_revision"`
	ExpectedStamp    string     `json:"expected_stamp"`
}

func DefaultCommandTrashFilePayload() CommandTrashFilePayload {
	return CommandTrashFilePayload{
		Worktree:         WorktreeID{},
		Path:             "",
		ExpectedRevision: "",
		ExpectedStamp:    "",
	}
}
func (value *CommandTrashFilePayload) UnmarshalJSON(data []byte) error {
	type plain CommandTrashFilePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "expected_revision", "expected_stamp"}, false); err != nil {
		return err
	}
	*value = CommandTrashFilePayload(decoded)
	return nil
}
func (value CommandTrashFilePayload) MarshalJSON() ([]byte, error) {
	type plain CommandTrashFilePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandPutCredentialPayload struct {
	ID               CredentialID `json:"id"`
	Provider         ProviderID   `json:"provider"`
	ExpectedRevision uint64       `json:"expected_revision"`
	Secret           Secret       `json:"secret"`
	ExpiresAtMs      *uint64      `json:"expires_at_ms"`
}

func DefaultCommandPutCredentialPayload() CommandPutCredentialPayload {
	return CommandPutCredentialPayload{
		ID:               CredentialID{},
		Provider:         ProviderID{},
		ExpectedRevision: 0,
		Secret:           Secret{},
		ExpiresAtMs:      nil,
	}
}
func (value *CommandPutCredentialPayload) UnmarshalJSON(data []byte) error {
	type plain CommandPutCredentialPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "provider", "expected_revision", "secret"}, false); err != nil {
		return err
	}
	*value = CommandPutCredentialPayload(decoded)
	return nil
}
func (value CommandPutCredentialPayload) MarshalJSON() ([]byte, error) {
	type plain CommandPutCredentialPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRevokeCredentialPayload struct {
	ID               CredentialID `json:"id"`
	ExpectedRevision uint64       `json:"expected_revision"`
}

func DefaultCommandRevokeCredentialPayload() CommandRevokeCredentialPayload {
	return CommandRevokeCredentialPayload{
		ID:               CredentialID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRevokeCredentialPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRevokeCredentialPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRevokeCredentialPayload(decoded)
	return nil
}
func (value CommandRevokeCredentialPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRevokeCredentialPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRegisterProjectPayload struct {
	Name string `json:"name"`
	Path string `json:"path"`
}

func DefaultCommandRegisterProjectPayload() CommandRegisterProjectPayload {
	return CommandRegisterProjectPayload{
		Name: "",
		Path: "",
	}
}
func (value *CommandRegisterProjectPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRegisterProjectPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "path"}, false); err != nil {
		return err
	}
	*value = CommandRegisterProjectPayload(decoded)
	return nil
}
func (value CommandRegisterProjectPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRegisterProjectPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandUpdateProjectPayload struct {
	Expected   Project            `json:"expected"`
	Name       string             `json:"name"`
	Path       string             `json:"path"`
	Appearance ProjectsAppearance `json:"appearance"`
}

func DefaultCommandUpdateProjectPayload() CommandUpdateProjectPayload {
	return CommandUpdateProjectPayload{
		Expected:   DefaultProject(),
		Name:       "",
		Path:       "",
		Appearance: DefaultProjectsAppearance(),
	}
}
func (value *CommandUpdateProjectPayload) UnmarshalJSON(data []byte) error {
	type plain CommandUpdateProjectPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected", "name", "path", "appearance"}, false); err != nil {
		return err
	}
	*value = CommandUpdateProjectPayload(decoded)
	return nil
}
func (value CommandUpdateProjectPayload) MarshalJSON() ([]byte, error) {
	type plain CommandUpdateProjectPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRemoveProjectPayload struct {
	Expected Project `json:"expected"`
}

func DefaultCommandRemoveProjectPayload() CommandRemoveProjectPayload {
	return CommandRemoveProjectPayload{
		Expected: DefaultProject(),
	}
}
func (value *CommandRemoveProjectPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveProjectPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected"}, false); err != nil {
		return err
	}
	*value = CommandRemoveProjectPayload(decoded)
	return nil
}
func (value CommandRemoveProjectPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveProjectPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetDefaultsPayload struct {
	ExpectedRevision uint64        `json:"expected_revision"`
	Config           SessionConfig `json:"config"`
}

func DefaultCommandSetDefaultsPayload() CommandSetDefaultsPayload {
	return CommandSetDefaultsPayload{
		ExpectedRevision: 0,
		Config:           DefaultSessionConfig(),
	}
}
func (value *CommandSetDefaultsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetDefaultsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected_revision", "config"}, false); err != nil {
		return err
	}
	*value = CommandSetDefaultsPayload(decoded)
	return nil
}
func (value CommandSetDefaultsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetDefaultsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandBindConnectionSessionPayload struct {
	Session          SessionID          `json:"session"`
	ExpectedRevision uint64             `json:"expected_revision"`
	Resource         ConnectionResource `json:"resource"`
}

func DefaultCommandBindConnectionSessionPayload() CommandBindConnectionSessionPayload {
	return CommandBindConnectionSessionPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Resource:         ConnectionResource{},
	}
}
func (value *CommandBindConnectionSessionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandBindConnectionSessionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "resource"}, false); err != nil {
		return err
	}
	*value = CommandBindConnectionSessionPayload(decoded)
	return nil
}
func (value CommandBindConnectionSessionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandBindConnectionSessionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateSessionPayload struct {
	Project  *ProjectID     `json:"project"`
	Worktree *WorktreeID    `json:"worktree,omitempty"`
	Config   *SessionConfig `json:"config"`
}

func DefaultCommandCreateSessionPayload() CommandCreateSessionPayload {
	return CommandCreateSessionPayload{
		Project:  nil,
		Worktree: nil,
		Config:   nil,
	}
}
func (value *CommandCreateSessionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateSessionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = CommandCreateSessionPayload(decoded)
	return nil
}
func (value CommandCreateSessionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateSessionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandReadWorktreeCatalogPayload struct {
	Worktree WorktreeID `json:"worktree"`
}

func DefaultCommandReadWorktreeCatalogPayload() CommandReadWorktreeCatalogPayload {
	return CommandReadWorktreeCatalogPayload{
		Worktree: WorktreeID{},
	}
}
func (value *CommandReadWorktreeCatalogPayload) UnmarshalJSON(data []byte) error {
	type plain CommandReadWorktreeCatalogPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree"}, false); err != nil {
		return err
	}
	*value = CommandReadWorktreeCatalogPayload(decoded)
	return nil
}
func (value CommandReadWorktreeCatalogPayload) MarshalJSON() ([]byte, error) {
	type plain CommandReadWorktreeCatalogPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandCreateSessionAtPayload struct {
	Target           NodeID         `json:"target"`
	Project          *ProjectID     `json:"project"`
	Worktree         *WorktreeID    `json:"worktree"`
	Config           *SessionConfig `json:"config"`
	ProviderRevision uint64         `json:"provider_revision"`
}

func DefaultCommandCreateSessionAtPayload() CommandCreateSessionAtPayload {
	return CommandCreateSessionAtPayload{
		Target:           NodeID{},
		Project:          nil,
		Worktree:         nil,
		Config:           nil,
		ProviderRevision: 0,
	}
}
func (value *CommandCreateSessionAtPayload) UnmarshalJSON(data []byte) error {
	type plain CommandCreateSessionAtPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"target", "config", "provider_revision"}, false); err != nil {
		return err
	}
	*value = CommandCreateSessionAtPayload(decoded)
	return nil
}
func (value CommandCreateSessionAtPayload) MarshalJSON() ([]byte, error) {
	type plain CommandCreateSessionAtPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandRenameSessionPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Title            string    `json:"title"`
}

func DefaultCommandRenameSessionPayload() CommandRenameSessionPayload {
	return CommandRenameSessionPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Title:            "",
	}
}
func (value *CommandRenameSessionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRenameSessionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "title"}, false); err != nil {
		return err
	}
	*value = CommandRenameSessionPayload(decoded)
	return nil
}
func (value CommandRenameSessionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRenameSessionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetSessionReadPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Read             bool      `json:"read"`
}

func DefaultCommandSetSessionReadPayload() CommandSetSessionReadPayload {
	return CommandSetSessionReadPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Read:             false,
	}
}
func (value *CommandSetSessionReadPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionReadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "read"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionReadPayload(decoded)
	return nil
}
func (value CommandSetSessionReadPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionReadPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetSessionArchivedPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
	Archived         bool      `json:"archived"`
}

func DefaultCommandSetSessionArchivedPayload() CommandSetSessionArchivedPayload {
	return CommandSetSessionArchivedPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Archived:         false,
	}
}
func (value *CommandSetSessionArchivedPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionArchivedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "archived"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionArchivedPayload(decoded)
	return nil
}
func (value CommandSetSessionArchivedPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionArchivedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandArchiveSessionsPayload struct {
	Expected []SessionRevision `json:"expected"`
}

func DefaultCommandArchiveSessionsPayload() CommandArchiveSessionsPayload {
	return CommandArchiveSessionsPayload{
		Expected: []SessionRevision{},
	}
}
func (value *CommandArchiveSessionsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandArchiveSessionsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected"}, false); err != nil {
		return err
	}
	*value = CommandArchiveSessionsPayload(decoded)
	return nil
}
func (value CommandArchiveSessionsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandArchiveSessionsPayload
	encoded := plain(value)
	encoded.Expected = normalizeCollections(encoded.Expected)
	return json.Marshal(encoded)
}

type CommandRemoveSessionsPayload struct {
	Expected []SessionRevision `json:"expected"`
}

func DefaultCommandRemoveSessionsPayload() CommandRemoveSessionsPayload {
	return CommandRemoveSessionsPayload{
		Expected: []SessionRevision{},
	}
}
func (value *CommandRemoveSessionsPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveSessionsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"expected"}, false); err != nil {
		return err
	}
	*value = CommandRemoveSessionsPayload(decoded)
	return nil
}
func (value CommandRemoveSessionsPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveSessionsPayload
	encoded := plain(value)
	encoded.Expected = normalizeCollections(encoded.Expected)
	return json.Marshal(encoded)
}

type CommandSetSessionOrderPayload struct {
	Project  ProjectID   `json:"project"`
	Expected []SessionID `json:"expected"`
	Sessions []SessionID `json:"sessions"`
}

func DefaultCommandSetSessionOrderPayload() CommandSetSessionOrderPayload {
	return CommandSetSessionOrderPayload{
		Project:  ProjectID{},
		Expected: []SessionID{},
		Sessions: []SessionID{},
	}
}
func (value *CommandSetSessionOrderPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionOrderPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"project", "expected", "sessions"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionOrderPayload(decoded)
	return nil
}
func (value CommandSetSessionOrderPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionOrderPayload
	encoded := plain(value)
	encoded.Expected = normalizeCollections(encoded.Expected)
	encoded.Sessions = normalizeCollections(encoded.Sessions)
	return json.Marshal(encoded)
}

type CommandRemoveSessionPayload struct {
	Session          SessionID `json:"session"`
	ExpectedRevision uint64    `json:"expected_revision"`
}

func DefaultCommandRemoveSessionPayload() CommandRemoveSessionPayload {
	return CommandRemoveSessionPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandRemoveSessionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandRemoveSessionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandRemoveSessionPayload(decoded)
	return nil
}
func (value CommandRemoveSessionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandRemoveSessionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandMoveConversationPayload struct {
	Session          SessionID  `json:"session"`
	Worktree         WorktreeID `json:"worktree"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandMoveConversationPayload() CommandMoveConversationPayload {
	return CommandMoveConversationPayload{
		Session:          SessionID{},
		Worktree:         WorktreeID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandMoveConversationPayload) UnmarshalJSON(data []byte) error {
	type plain CommandMoveConversationPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "worktree", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandMoveConversationPayload(decoded)
	return nil
}
func (value CommandMoveConversationPayload) MarshalJSON() ([]byte, error) {
	type plain CommandMoveConversationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandForkConversationAtPayload struct {
	Session          SessionID  `json:"session"`
	Worktree         WorktreeID `json:"worktree"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultCommandForkConversationAtPayload() CommandForkConversationAtPayload {
	return CommandForkConversationAtPayload{
		Session:          SessionID{},
		Worktree:         WorktreeID{},
		ExpectedRevision: 0,
	}
}
func (value *CommandForkConversationAtPayload) UnmarshalJSON(data []byte) error {
	type plain CommandForkConversationAtPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "worktree", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandForkConversationAtPayload(decoded)
	return nil
}
func (value CommandForkConversationAtPayload) MarshalJSON() ([]byte, error) {
	type plain CommandForkConversationAtPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetSessionConfigPayload struct {
	Session          SessionID     `json:"session"`
	ExpectedRevision uint64        `json:"expected_revision"`
	Config           SessionConfig `json:"config"`
}

func DefaultCommandSetSessionConfigPayload() CommandSetSessionConfigPayload {
	return CommandSetSessionConfigPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Config:           DefaultSessionConfig(),
	}
}
func (value *CommandSetSessionConfigPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionConfigPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "config"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionConfigPayload(decoded)
	return nil
}
func (value CommandSetSessionConfigPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionConfigPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandSetSessionRolesPayload struct {
	Session          SessionID       `json:"session"`
	ExpectedRevision uint64          `json:"expected_revision"`
	Roles            []RoleReference `json:"roles"`
}

func DefaultCommandSetSessionRolesPayload() CommandSetSessionRolesPayload {
	return CommandSetSessionRolesPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Roles:            []RoleReference{},
	}
}
func (value *CommandSetSessionRolesPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionRolesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "roles"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionRolesPayload(decoded)
	return nil
}
func (value CommandSetSessionRolesPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionRolesPayload
	encoded := plain(value)
	encoded.Roles = normalizeCollections(encoded.Roles)
	return json.Marshal(encoded)
}

type CommandSetSessionExecutionPayload struct {
	Session          SessionID             `json:"session"`
	ExpectedRevision uint64                `json:"expected_revision"`
	Execution        ConversationExecution `json:"execution"`
	Team             *TeamSelection        `json:"team"`
	Models           map[RoleID]RoleModel  `json:"models,omitempty"`
}

func DefaultCommandSetSessionExecutionPayload() CommandSetSessionExecutionPayload {
	return CommandSetSessionExecutionPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Execution:        ConversationExecutionTeam,
		Team:             nil,
		Models:           map[RoleID]RoleModel{},
	}
}
func (value *CommandSetSessionExecutionPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionExecutionPayload
	decoded := plain{}
	decoded.Execution = ConversationExecutionTeam
	decoded.Models = map[RoleID]RoleModel{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionExecutionPayload(decoded)
	return nil
}
func (value CommandSetSessionExecutionPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionExecutionPayload
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

type CommandSetSessionMemberModelPayload struct {
	Session          SessionID  `json:"session"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Role             RoleID     `json:"role"`
	Model            *RoleModel `json:"model"`
}

func DefaultCommandSetSessionMemberModelPayload() CommandSetSessionMemberModelPayload {
	return CommandSetSessionMemberModelPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Role:             RoleID{},
		Model:            nil,
	}
}
func (value *CommandSetSessionMemberModelPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetSessionMemberModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "role"}, false); err != nil {
		return err
	}
	*value = CommandSetSessionMemberModelPayload(decoded)
	return nil
}
func (value CommandSetSessionMemberModelPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSetSessionMemberModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CommandQueueTurnPayload struct {
	Session          SessionID         `json:"session"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Message          ConversationInput `json:"message"`
}

func DefaultCommandQueueTurnPayload() CommandQueueTurnPayload {
	return CommandQueueTurnPayload{
		Session:          SessionID{},
		ExpectedRevision: 0,
		Message:          DefaultConversationInput(),
	}
}
func (value *CommandQueueTurnPayload) UnmarshalJSON(data []byte) error {
	type plain CommandQueueTurnPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "expected_revision", "message"}, false); err != nil {
		return err
	}
	*value = CommandQueueTurnPayload(decoded)
	return nil
}
func (value CommandQueueTurnPayload) MarshalJSON() ([]byte, error) {
	type plain CommandQueueTurnPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var commandVariants = variantFactories{"write_artifact": func() any { return new(ArtifactWrite) },
	"read_artifact":                    func() any { return new(CommandReadArtifactPayload) },
	"list_artifacts":                   func() any { return new(CommandListArtifactsPayload) },
	"list_artifact_revisions":          func() any { return new(CommandListArtifactRevisionsPayload) },
	"plugin_transaction":               func() any { return new(CommandPluginTransactionPayload) },
	"call_plugin":                      func() any { return new(CommandCallPluginPayload) },
	"read_plugin_callback":             func() any { return new(CommandReadPluginCallbackPayload) },
	"cancel_plugin_call":               func() any { return new(CommandCancelPluginCallPayload) },
	"report_diagnostic":                func() any { return new(NotificationSystemSystemDiagnostic) },
	"publish_notification":             func() any { return new(CommandPublishNotificationPayload) },
	"mark_notification_read":           func() any { return new(CommandMarkNotificationReadPayload) },
	"mark_notifications_read":          func() any { return new(CommandMarkNotificationsReadPayload) },
	"dismiss_read_notifications":       func() any { return new(CommandDismissReadNotificationsPayload) },
	"dismiss_notifications":            func() any { return new(CommandDismissNotificationsPayload) },
	"dispatch":                         func() any { return new(CommandDispatchPayload) },
	"stop_dispatch_turn":               func() any { return new(CommandStopDispatchTurnPayload) },
	"use_browser":                      func() any { return new(CommandUseBrowserPayload) },
	"use_external_browser":             func() any { return new(CommandUseExternalBrowserPayload) },
	"complete_browser":                 func() any { return new(CommandCompleteBrowserPayload) },
	"read_plugin_value":                func() any { return new(CommandReadPluginValuePayload) },
	"read_plugin_conversation_value":   func() any { return new(CommandReadPluginConversationValuePayload) },
	"list_plugin_keys":                 func() any { return new(CommandListPluginKeysPayload) },
	"write_plugin_value":               func() any { return new(CommandWritePluginValuePayload) },
	"write_indexed_plugin_value":       func() any { return new(CommandWriteIndexedPluginValuePayload) },
	"search_plugin_values":             func() any { return new(PluginStorageSearch) },
	"remove_plugin_value":              func() any { return new(CommandRemovePluginValuePayload) },
	"write_plugin_conversation_value":  func() any { return new(CommandWritePluginConversationValuePayload) },
	"remove_plugin_conversation_value": func() any { return new(CommandRemovePluginConversationValuePayload) },
	"list_plugin_models":               nil,
	"resolve_plugin_model":             func() any { return new(CommandResolvePluginModelPayload) },
	"request_plugin_http":              func() any { return new(PluginHttpRequest) },
	"request_provider_http":            func() any { return new(CommandRequestProviderHttpPayload) },
	"execute_provider_http":            func() any { return new(CommandExecuteProviderHttpPayload) },
	"fetch_web":                        func() any { return new(WebRequest) },
	"cancel_web_fetch":                 func() any { return new(CommandCancelWebFetchPayload) },
	"search_web":                       func() any { return new(SearchRequest) },
	"cancel_web_search":                func() any { return new(CommandCancelWebSearchPayload) },
	"test_search_driver":               func() any { return new(CommandTestSearchDriverPayload) },
	"generate_plugin_text":             func() any { return new(CommandGeneratePluginTextPayload) },
	"browse_files":                     func() any { return new(CommandBrowseFilesPayload) },
	"manage_files":                     func() any { return new(FileBrowserAction) },
	"open_port":                        func() any { return new(CommandOpenPortPayload) },
	"cancel_port":                      func() any { return new(CommandCancelPortPayload) },
	"list_databases":                   nil,
	"save_database":                    func() any { return new(CommandSaveDatabasePayload) },
	"remove_database":                  func() any { return new(CommandRemoveDatabasePayload) },
	"test_database":                    func() any { return new(CommandTestDatabasePayload) },
	"check_database":                   func() any { return new(CommandCheckDatabasePayload) },
	"browse_database":                  func() any { return new(CommandBrowseDatabasePayload) },
	"query_database":                   func() any { return new(CommandQueryDatabasePayload) },
	"cancel_database":                  func() any { return new(CommandCancelDatabasePayload) },
	"modify_ssh_file":                  func() any { return new(CommandModifySshFilePayload) },
	"browse_ssh_directory":             func() any { return new(CommandBrowseSshDirectoryPayload) },
	"download_ssh_file":                func() any { return new(CommandDownloadSshFilePayload) },
	"stage_ssh_upload":                 func() any { return new(SshUploadSpec) },
	"finish_ssh_upload":                func() any { return new(CommandFinishSshUploadPayload) },
	"list_ssh":                         nil,
	"list_ssh_terminals":               func() any { return new(CommandListSshTerminalsPayload) },
	"save_ssh":                         func() any { return new(CommandSaveSshPayload) },
	"remove_ssh":                       func() any { return new(CommandRemoveSshPayload) },
	"trust_ssh":                        func() any { return new(CommandTrustSshPayload) },
	"list_wsl_distributions":           nil,
	"install_wsl_host":                 func() any { return new(CommandInstallWslHostPayload) },
	"read_wsl_host_install":            func() any { return new(CommandReadWslHostInstallPayload) },
	"cancel_wsl_host_install":          func() any { return new(CommandCancelWslHostInstallPayload) },
	"install_host":                     func() any { return new(CommandInstallHostPayload) },
	"update_ssh_host":                  func() any { return new(CommandUpdateSshHostPayload) },
	"read_host_install":                func() any { return new(CommandReadHostInstallPayload) },
	"check_ssh":                        func() any { return new(CommandCheckSshPayload) },
	"open_ssh_terminal":                func() any { return new(CommandOpenSshTerminalPayload) },
	"close_ssh_terminal":               func() any { return new(CommandCloseSshTerminalPayload) },
	"run_ssh":                          func() any { return new(CommandRunSshPayload) },
	"cancel_ssh":                       func() any { return new(CommandCancelSshPayload) },
	"transfer_ssh":                     func() any { return new(CommandTransferSshPayload) },
	"list_plugins":                     nil,
	"search_skill_catalog":             func() any { return new(CommandSearchSkillCatalogPayload) },
	"search_plugin_catalog":            func() any { return new(CommandSearchPluginCatalogPayload) },
	"read_catalog_plugin":              func() any { return new(CommandReadCatalogPluginPayload) },
	"read_catalog_plugin_info":         func() any { return new(CommandReadCatalogPluginInfoPayload) },
	"inspect_plugin_source":            func() any { return new(CommandInspectPluginSourcePayload) },
	"check_plugin_update":              func() any { return new(CommandCheckPluginUpdatePayload) },
	"check_plugin_updates":             func() any { return new(CommandCheckPluginUpdatesPayload) },
	"dismiss_installation":             func() any { return new(CommandDismissInstallationPayload) },
	"read_installations":               func() any { return new(CommandReadInstallationsPayload) },
	"install_plugin_source":            func() any { return new(CommandInstallPluginSourcePayload) },
	"install_bundled_plugin":           func() any { return new(CommandInstallBundledPluginPayload) },
	"read_skill_document":              func() any { return new(CommandReadSkillDocumentPayload) },
	"discover_skills":                  func() any { return new(CommandDiscoverSkillsPayload) },
	"discover_skill_upload":            func() any { return new(CommandDiscoverSkillUploadPayload) },
	"discover_local_skills":            func() any { return new(CommandDiscoverLocalSkillsPayload) },
	"install_skill_upload":             func() any { return new(CommandInstallSkillUploadPayload) },
	"discover_registry_skill":          func() any { return new(CommandDiscoverRegistrySkillPayload) },
	"install_registry_skill":           func() any { return new(CommandInstallRegistrySkillPayload) },
	"install_skill":                    func() any { return new(CommandInstallSkillPayload) },
	"install_mcp":                      func() any { return new(CommandInstallMcpPayload) },
	"upload_plugin":                    func() any { return new(PluginUploadSpec) },
	"inspect_plugin_upload":            func() any { return new(CommandInspectPluginUploadPayload) },
	"install_plugin_upload":            func() any { return new(CommandInstallPluginUploadPayload) },
	"read_plugin":                      func() any { return new(CommandReadPluginPayload) },
	"read_plugin_version":              func() any { return new(CommandReadPluginVersionPayload) },
	"read_plugin_settings":             func() any { return new(CommandReadPluginSettingsPayload) },
	"read_plugin_mcp":                  func() any { return new(CommandReadPluginMcpPayload) },
	"set_plugin_mcp_allowed":           func() any { return new(CommandSetPluginMcpAllowedPayload) },
	"check_plugin_mcp":                 func() any { return new(CommandCheckPluginMcpPayload) },
	"list_connector_types":             nil,
	"inspect_connector_cli":            func() any { return new(CommandInspectConnectorCliPayload) },
	"list_connections":                 nil,
	"start_connector_login":            func() any { return new(CommandStartConnectorLoginPayload) },
	"submit_connector_input":           func() any { return new(CommandSubmitConnectorInputPayload) },
	"cancel_connector_login":           func() any { return new(CommandCancelConnectorLoginPayload) },
	"update_connection":                func() any { return new(CommandUpdateConnectionPayload) },
	"set_connection_enabled":           func() any { return new(CommandSetConnectionEnabledPayload) },
	"disconnect_connection":            func() any { return new(CommandDisconnectConnectionPayload) },
	"remove_connection":                func() any { return new(CommandRemoveConnectionPayload) },
	"confirm_connector_consent":        func() any { return new(CommandConfirmConnectorConsentPayload) },
	"probe_connection":                 func() any { return new(CommandProbeConnectionPayload) },
	"save_plugin_mcp":                  func() any { return new(CommandSavePluginMcpPayload) },
	"read_plugin_secret":               func() any { return new(CommandReadPluginSecretPayload) },
	"read_plugin_connection":           func() any { return new(CommandReadPluginConnectionPayload) },
	"save_plugin_settings":             func() any { return new(CommandSavePluginSettingsPayload) },
	"read_plugin_view":                 func() any { return new(CommandReadPluginViewPayload) },
	"install_plugin":                   func() any { return new(CommandInstallPluginPayload) },
	"set_plugin_enabled":               func() any { return new(CommandSetPluginEnabledPayload) },
	"remove_plugin":                    func() any { return new(CommandRemovePluginPayload) },
	"list_roles":                       nil,
	"put_role":                         func() any { return new(CommandPutRolePayload) },
	"remove_role":                      func() any { return new(CommandRemoveRolePayload) },
	"list_teams":                       nil,
	"put_team":                         func() any { return new(CommandPutTeamPayload) },
	"remove_team":                      func() any { return new(CommandRemoveTeamPayload) },
	"read_compaction_settings":         nil,
	"save_compaction_settings":         func() any { return new(ConversationCompactionSettings) },
	"read_team_settings":               nil,
	"save_team_settings":               func() any { return new(TeamSettings) },
	"list_providers":                   nil,
	"read_provider_key":                func() any { return new(CommandReadProviderKeyPayload) },
	"begin_provider_login":             func() any { return new(CommandBeginProviderLoginPayload) },
	"complete_provider_login":          func() any { return new(CommandCompleteProviderLoginPayload) },
	"cancel_provider_login":            func() any { return new(CommandCancelProviderLoginPayload) },
	"read_catalog_status":              nil,
	"list_channel_presets":             func() any { return new(CommandListChannelPresetsPayload) },
	"refresh_model_catalog":            nil,
	"read_model_catalog":               func() any { return new(ConversationCatalogQuery) },
	"read_model_scores":                func() any { return new([]ConversationCatalogScoresModelRef) },
	"discover_models":                  func() any { return new(ConversationDiscoverySource) },
	"validate_provider":                func() any { return new(CommandValidateProviderPayload) },
	"put_provider":                     func() any { return new(CommandPutProviderPayload) },
	"read_session":                     func() any { return new(CommandReadSessionPayload) },
	"list_associated_sessions":         func() any { return new(CommandListAssociatedSessionsPayload) },
	"read_conversation":                func() any { return new(CommandReadConversationPayload) },
	"read_conversation_runs":           func() any { return new(CommandReadConversationRunsPayload) },
	"read_conversation_run":            func() any { return new(CommandReadConversationRunPayload) },
	"read_usage":                       func() any { return new(UsageQuery) },
	"read_activity_catalog":            nil,
	"read_activity":                    func() any { return new(CommandReadActivityPayload) },
	"read_turn_capture":                func() any { return new(CommandReadTurnCapturePayload) },
	"read_turn":                        func() any { return new(CommandReadTurnPayload) },
	"read_tool_detail":                 func() any { return new(CommandReadToolDetailPayload) },
	"list_conversation_assets":         func() any { return new(CommandListConversationAssetsPayload) },
	"search_conversation":              func() any { return new(CommandSearchConversationPayload) },
	"list_file_checkpoints":            func() any { return new(CommandListFileCheckpointsPayload) },
	"read_file_checkpoint":             func() any { return new(CommandReadFileCheckpointPayload) },
	"read_turn_diff":                   func() any { return new(CommandReadTurnDiffPayload) },
	"read_turn_hunks":                  func() any { return new(CommandReadTurnHunksPayload) },
	"apply_turn_hunk":                  func() any { return new(ConversationCheckpointHunkSelection) },
	"restore_file_checkpoint":          func() any { return new(CommandRestoreFileCheckpointPayload) },
	"fork_conversation":                func() any { return new(CommandForkConversationPayload) },
	"rewind_conversation":              func() any { return new(CommandRewindConversationPayload) },
	"replace_turn":                     func() any { return new(CommandReplaceTurnPayload) },
	"save_provider":                    func() any { return new(CommandSaveProviderPayload) },
	"remove_provider":                  func() any { return new(CommandRemoveProviderPayload) },
	"submit_turn":                      func() any { return new(CommandSubmitTurnPayload) },
	"submit_task":                      func() any { return new(CommandSubmitTaskPayload) },
	"guide_turn":                       func() any { return new(CommandGuideTurnPayload) },
	"deliver_background":               func() any { return new(CommandDeliverBackgroundPayload) },
	"continue_turn":                    func() any { return new(CommandContinueTurnPayload) },
	"compact_context":                  func() any { return new(CommandCompactContextPayload) },
	"start_queued_turn":                func() any { return new(CommandStartQueuedTurnPayload) },
	"stop_turn":                        func() any { return new(CommandStopTurnPayload) },
	"run_command":                      func() any { return new(CommandRunCommandPayload) },
	"list_commands":                    func() any { return new(CommandListCommandsPayload) },
	"read_command":                     func() any { return new(CommandReadCommandPayload) },
	"read_command_page":                func() any { return new(CommandReadCommandPagePayload) },
	"read_tool_output":                 func() any { return new(CommandReadToolOutputPayload) },
	"stop_commands":                    func() any { return new(CommandStopCommandsPayload) },
	"stop_command":                     func() any { return new(CommandStopCommandPayload) },
	"write_command":                    func() any { return new(CommandWriteCommandPayload) },
	"resolve_approval":                 func() any { return new(CommandResolveApprovalPayload) },
	"resolve_question":                 func() any { return new(CommandResolveQuestionPayload) },
	"read_queued_turn":                 func() any { return new(CommandReadQueuedTurnPayload) },
	"edit_queued_turn":                 func() any { return new(CommandEditQueuedTurnPayload) },
	"remove_queued_turn":               func() any { return new(CommandRemoveQueuedTurnPayload) },
	"send_queued_turn":                 func() any { return new(CommandSendQueuedTurnPayload) },
	"guide_queued_turn":                func() any { return new(CommandGuideQueuedTurnPayload) },
	"withdraw_guidance":                func() any { return new(CommandWithdrawGuidancePayload) },
	"move_queued_turn":                 func() any { return new(CommandMoveQueuedTurnPayload) },
	"set_queue_paused":                 func() any { return new(CommandSetQueuePausedPayload) },
	"read_terminal_settings":           nil,
	"read_power_settings":              nil,
	"read_telemetry_settings":          nil,
	"save_telemetry_settings":          func() any { return new(TelemetryUpdate) },
	"reset_telemetry_identity":         func() any { return new(CommandResetTelemetryIdentityPayload) },
	"send_diagnostics":                 nil,
	"read_network_settings":            nil,
	"list_network_presets":             func() any { return new(CommandListNetworkPresetsPayload) },
	"probe_network":                    func() any { return new(CommandProbeNetworkPayload) },
	"save_network_settings":            func() any { return new(CommandSaveNetworkSettingsPayload) },
	"read_sandbox_settings":            nil,
	"save_sandbox_settings":            func() any { return new(SandboxSettings) },
	"read_search_settings":             nil,
	"save_search_settings":             func() any { return new(SearchUpdate) },
	"read_backup_settings":             nil,
	"save_backup_settings":             func() any { return new(BackupUpdate) },
	"list_backups":                     nil,
	"export_backup":                    func() any { return new(CommandExportBackupPayload) },
	"download_backup":                  func() any { return new(CommandDownloadBackupPayload) },
	"upload_backup":                    func() any { return new(CommandUploadBackupPayload) },
	"restore_backup_file":              func() any { return new(CommandRestoreBackupFilePayload) },
	"restore_backup":                   func() any { return new(CommandRestoreBackupPayload) },
	"read_python_settings":             nil,
	"save_python_settings":             func() any { return new(PythonSettings) },
	"decide":                           func() any { return new(DecisionRequest) },
	"read_local_model_settings":        nil,
	"save_local_model_settings":        func() any { return new(ModelsLocalSettings) },
	"read_local_models":                func() any { return new(CommandReadLocalModelsPayload) },
	"refresh_local_model":              func() any { return new(CommandRefreshLocalModelPayload) },
	"register_local_model":             func() any { return new(CommandRegisterLocalModelPayload) },
	"install_local_model":              func() any { return new(CommandInstallLocalModelPayload) },
	"delete_local_model":               func() any { return new(CommandDeleteLocalModelPayload) },
	"cancel_local_model":               func() any { return new(CommandCancelLocalModelPayload) },
	"list_shells":                      nil,
	"save_terminal_settings":           func() any { return new(TerminalSettings) },
	"save_power_settings":              func() any { return new(PowerSettings) },
	"list_media_models":                nil,
	"read_media_settings":              nil,
	"save_media_settings":              func() any { return new(MediaSettings) },
	"search_models":                    func() any { return new(ModelsSearch) },
	"read_model_versions":              func() any { return new(CommandReadModelVersionsPayload) },
	"resolve_model":                    func() any { return new(CommandResolveModelPayload) },
	"query_knowledge":                  func() any { return new(KnowledgeQuery) },
	"read_knowledge_citation":          func() any { return new(KnowledgeCitationRequest) },
	"list_knowledge_collections":       nil,
	"create_knowledge_collection":      func() any { return new(CommandCreateKnowledgeCollectionPayload) },
	"update_knowledge_collection":      func() any { return new(CommandUpdateKnowledgeCollectionPayload) },
	"delete_knowledge_collection":      func() any { return new(CommandDeleteKnowledgeCollectionPayload) },
	"rebuild_knowledge_collection":     func() any { return new(CommandRebuildKnowledgeCollectionPayload) },
	"pause_knowledge_collection":       func() any { return new(CommandPauseKnowledgeCollectionPayload) },
	"create_terminal":                  func() any { return new(TerminalLaunch) },
	"open_terminal":                    func() any { return new(CommandOpenTerminalPayload) },
	"open_tool_terminal":               func() any { return new(CommandOpenToolTerminalPayload) },
	"list_terminal_tools":              func() any { return new(CommandListTerminalToolsPayload) },
	"close_terminal":                   func() any { return new(CommandCloseTerminalPayload) },
	"list_terminals":                   func() any { return new(CommandListTerminalsPayload) },
	"inspect_terminal":                 func() any { return new(CommandInspectTerminalPayload) },
	"claim_terminal":                   func() any { return new(CommandClaimTerminalPayload) },
	"input_terminal":                   func() any { return new(CommandInputTerminalPayload) },
	"resize_terminal":                  func() any { return new(CommandResizeTerminalPayload) },
	"set_terminal_appearance":          func() any { return new(CommandSetTerminalAppearancePayload) },
	"snapshot":                         nil,
	"inspect_host":                     nil,
	"read_host_metrics":                func() any { return new(CommandReadHostMetricsPayload) },
	"use_media":                        func() any { return new(CommandUseMediaPayload) },
	"read_computer_permissions":        nil,
	"request_computer_permission":      func() any { return new(CommandRequestComputerPermissionPayload) },
	"stop_host_process":                func() any { return new(CommandStopHostProcessPayload) },
	"inspect_request":                  func() any { return new(CommandInspectRequestPayload) },
	"inspect_git":                      func() any { return new(CommandInspectGitPayload) },
	"read_git_head":                    func() any { return new(CommandReadGitHeadPayload) },
	"list_git_branches":                func() any { return new(CommandListGitBranchesPayload) },
	"resolve_git_revision":             func() any { return new(CommandResolveGitRevisionPayload) },
	"list_worktrees":                   func() any { return new(CommandListWorktreesPayload) },
	"register_worktree":                func() any { return new(CommandRegisterWorktreePayload) },
	"create_worktree":                  func() any { return new(CommandCreateWorktreePayload) },
	"create_managed_worktree":          func() any { return new(CommandCreateManagedWorktreePayload) },
	"remove_worktree":                  func() any { return new(CommandRemoveWorktreePayload) },
	"create_git_branch":                func() any { return new(CommandCreateGitBranchPayload) },
	"rename_git_branch":                func() any { return new(CommandRenameGitBranchPayload) },
	"delete_git_branch":                func() any { return new(CommandDeleteGitBranchPayload) },
	"switch_git_branch":                func() any { return new(CommandSwitchGitBranchPayload) },
	"merge_git_branch":                 func() any { return new(CommandMergeGitBranchPayload) },
	"read_git_stash":                   func() any { return new(CommandReadGitStashPayload) },
	"read_git_output":                  func() any { return new(CommandReadGitOutputPayload) },
	"list_git_remote_tags":             func() any { return new(CommandListGitRemoteTagsPayload) },
	"read_git_log":                     func() any { return new(CommandReadGitLogPayload) },
	"read_git_commit":                  func() any { return new(CommandReadGitCommitPayload) },
	"list_git_changes":                 func() any { return new(CommandListGitChangesPayload) },
	"read_git_change":                  func() any { return new(CommandReadGitChangePayload) },
	"read_git_context":                 func() any { return new(CommandReadGitContextPayload) },
	"read_git_show":                    func() any { return new(CommandReadGitShowPayload) },
	"read_git_blame":                   func() any { return new(CommandReadGitBlamePayload) },
	"search_files":                     func() any { return new(CommandSearchFilesPayload) },
	"run_git_action":                   func() any { return new(CommandRunGitActionPayload) },
	"create_git_commit":                func() any { return new(CommandCreateGitCommitPayload) },
	"update_git_index":                 func() any { return new(CommandUpdateGitIndexPayload) },
	"read_git_diff":                    func() any { return new(CommandReadGitDiffPayload) },
	"list_directory":                   func() any { return new(CommandListDirectoryPayload) },
	"read_file":                        func() any { return new(CommandReadFilePayload) },
	"search_code":                      func() any { return new(CommandSearchCodePayload) },
	"outline_code":                     func() any { return new(CommandOutlineCodePayload) },
	"graph_code":                       func() any { return new(CommandGraphCodePayload) },
	"cancel_code_index_query":          func() any { return new(CommandCancelCodeIndexQueryPayload) },
	"read_code_index_settings":         nil,
	"read_hooks":                       func() any { return new(CommandReadHooksPayload) },
	"save_hooks":                       func() any { return new(CommandSaveHooksPayload) },
	"save_code_index_settings":         func() any { return new(CommandSaveCodeIndexSettingsPayload) },
	"read_file_lines":                  func() any { return new(CommandReadFileLinesPayload) },
	"read_image":                       func() any { return new(CommandReadImagePayload) },
	"find_files":                       func() any { return new(CommandFindFilesPayload) },
	"read_office":                      func() any { return new(CommandReadOfficePayload) },
	"read_notebook":                    func() any { return new(CommandReadNotebookPayload) },
	"edit_notebook":                    func() any { return new(CommandEditNotebookPayload) },
	"download_file":                    func() any { return new(CommandDownloadFilePayload) },
	"upload_file":                      func() any { return new(FileUploadSpec) },
	"upload_attachment":                func() any { return new(AttachmentSpec) },
	"finish_attachment_upload":         func() any { return new(CommandFinishAttachmentUploadPayload) },
	"read_attachment":                  func() any { return new(CommandReadAttachmentPayload) },
	"download_attachment":              func() any { return new(CommandDownloadAttachmentPayload) },
	"download_image":                   func() any { return new(CommandDownloadImagePayload) },
	"discard_attachment":               func() any { return new(CommandDiscardAttachmentPayload) },
	"finish_file_upload":               func() any { return new(CommandFinishFileUploadPayload) },
	"cancel_file_transfer":             func() any { return new(CommandCancelFileTransferPayload) },
	"write_file":                       func() any { return new(CommandWriteFilePayload) },
	"edit_file":                        func() any { return new(CommandEditFilePayload) },
	"delete_file":                      func() any { return new(CommandDeleteFilePayload) },
	"move_file":                        func() any { return new(CommandMoveFilePayload) },
	"apply_changes":                    func() any { return new(CommandApplyChangesPayload) },
	"create_directory":                 func() any { return new(CommandCreateDirectoryPayload) },
	"rename_entry":                     func() any { return new(CommandRenameEntryPayload) },
	"copy_entry":                       func() any { return new(CommandCopyEntryPayload) },
	"copy_entry_to":                    func() any { return new(CommandCopyEntryToPayload) },
	"move_entry_to":                    func() any { return new(CommandMoveEntryToPayload) },
	"trash_entry":                      func() any { return new(CommandTrashEntryPayload) },
	"trash_file":                       func() any { return new(CommandTrashFilePayload) },
	"list_credentials":                 nil,
	"put_credential":                   func() any { return new(CommandPutCredentialPayload) },
	"revoke_credential":                func() any { return new(CommandRevokeCredentialPayload) },
	"register_project":                 func() any { return new(CommandRegisterProjectPayload) },
	"create_project":                   func() any { return new(ProjectsDraft) },
	"update_project":                   func() any { return new(CommandUpdateProjectPayload) },
	"remove_project":                   func() any { return new(CommandRemoveProjectPayload) },
	"set_defaults":                     func() any { return new(CommandSetDefaultsPayload) },
	"bind_connection_session":          func() any { return new(CommandBindConnectionSessionPayload) },
	"create_session":                   func() any { return new(CommandCreateSessionPayload) },
	"start_session":                    func() any { return new(ConversationStart) },
	"read_project_catalog":             nil,
	"read_worktree_catalog":            func() any { return new(CommandReadWorktreeCatalogPayload) },
	"create_session_at":                func() any { return new(CommandCreateSessionAtPayload) },
	"import_session":                   func() any { return new(SessionImport) },
	"rename_session":                   func() any { return new(CommandRenameSessionPayload) },
	"set_session_read":                 func() any { return new(CommandSetSessionReadPayload) },
	"set_session_archived":             func() any { return new(CommandSetSessionArchivedPayload) },
	"archive_sessions":                 func() any { return new(CommandArchiveSessionsPayload) },
	"remove_sessions":                  func() any { return new(CommandRemoveSessionsPayload) },
	"set_session_order":                func() any { return new(CommandSetSessionOrderPayload) },
	"remove_session":                   func() any { return new(CommandRemoveSessionPayload) },
	"move_conversation":                func() any { return new(CommandMoveConversationPayload) },
	"fork_conversation_at":             func() any { return new(CommandForkConversationAtPayload) },
	"set_session_config":               func() any { return new(CommandSetSessionConfigPayload) },
	"set_session_roles":                func() any { return new(CommandSetSessionRolesPayload) },
	"set_session_team":                 func() any { return new(CommandSetSessionExecutionPayload) },
	"set_session_member_model":         func() any { return new(CommandSetSessionMemberModelPayload) },
	"queue_turn":                       func() any { return new(CommandQueueTurnPayload) }}

func (value Command) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, commandVariants)
}
func (value *Command) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", commandVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
func CommandKinds() []string {
	return append([]string{"write_artifact", "read_artifact", "list_artifacts", "list_artifact_revisions", "plugin_transaction", "call_plugin", "read_plugin_callback", "cancel_plugin_call", "report_diagnostic", "publish_notification", "mark_notification_read", "mark_notifications_read", "dismiss_read_notifications", "dismiss_notifications", "dispatch", "stop_dispatch_turn", "use_browser", "use_external_browser", "complete_browser", "read_plugin_value", "read_plugin_conversation_value", "list_plugin_keys", "write_plugin_value", "write_indexed_plugin_value", "search_plugin_values", "remove_plugin_value", "write_plugin_conversation_value", "remove_plugin_conversation_value", "list_plugin_models", "resolve_plugin_model", "request_plugin_http", "request_provider_http", "execute_provider_http", "fetch_web", "cancel_web_fetch", "search_web", "cancel_web_search", "test_search_driver", "generate_plugin_text", "browse_files", "manage_files", "open_port", "cancel_port", "list_databases", "save_database", "remove_database", "test_database", "check_database", "browse_database", "query_database", "cancel_database", "modify_ssh_file", "browse_ssh_directory", "download_ssh_file", "stage_ssh_upload", "finish_ssh_upload", "list_ssh", "list_ssh_terminals", "save_ssh", "remove_ssh", "trust_ssh", "list_wsl_distributions", "install_wsl_host", "read_wsl_host_install", "cancel_wsl_host_install", "install_host", "update_ssh_host", "read_host_install", "check_ssh", "open_ssh_terminal", "close_ssh_terminal", "run_ssh", "cancel_ssh", "transfer_ssh", "list_plugins", "search_skill_catalog", "search_plugin_catalog", "read_catalog_plugin", "read_catalog_plugin_info", "inspect_plugin_source", "check_plugin_update", "check_plugin_updates", "dismiss_installation", "read_installations", "install_plugin_source", "install_bundled_plugin", "read_skill_document", "discover_skills", "discover_skill_upload", "discover_local_skills", "install_skill_upload", "discover_registry_skill", "install_registry_skill", "install_skill", "install_mcp", "upload_plugin", "inspect_plugin_upload", "install_plugin_upload", "read_plugin", "read_plugin_version", "read_plugin_settings", "read_plugin_mcp", "set_plugin_mcp_allowed", "check_plugin_mcp", "list_connector_types", "inspect_connector_cli", "list_connections", "start_connector_login", "submit_connector_input", "cancel_connector_login", "update_connection", "set_connection_enabled", "disconnect_connection", "remove_connection", "confirm_connector_consent", "probe_connection", "save_plugin_mcp", "read_plugin_secret", "read_plugin_connection", "save_plugin_settings", "read_plugin_view", "install_plugin", "set_plugin_enabled", "remove_plugin", "list_roles", "put_role", "remove_role", "list_teams", "put_team", "remove_team", "read_compaction_settings", "save_compaction_settings", "read_team_settings", "save_team_settings", "list_providers", "read_provider_key", "begin_provider_login", "complete_provider_login", "cancel_provider_login", "read_catalog_status", "list_channel_presets", "refresh_model_catalog", "read_model_catalog", "read_model_scores", "discover_models", "validate_provider", "put_provider", "read_session", "list_associated_sessions", "read_conversation", "read_conversation_runs", "read_conversation_run", "read_usage", "read_activity_catalog", "read_activity", "read_turn_capture", "read_turn", "read_tool_detail", "list_conversation_assets", "search_conversation", "list_file_checkpoints", "read_file_checkpoint", "read_turn_diff", "read_turn_hunks", "apply_turn_hunk", "restore_file_checkpoint", "fork_conversation", "rewind_conversation", "replace_turn", "save_provider", "remove_provider", "submit_turn", "submit_task", "guide_turn", "deliver_background", "continue_turn", "compact_context", "start_queued_turn", "stop_turn", "run_command", "list_commands", "read_command", "read_command_page", "read_tool_output", "stop_commands", "stop_command", "write_command", "resolve_approval", "resolve_question", "read_queued_turn", "edit_queued_turn", "remove_queued_turn", "send_queued_turn", "guide_queued_turn", "withdraw_guidance", "move_queued_turn", "set_queue_paused", "read_terminal_settings", "read_power_settings", "read_telemetry_settings", "save_telemetry_settings", "reset_telemetry_identity", "send_diagnostics", "read_network_settings", "list_network_presets", "probe_network", "save_network_settings", "read_sandbox_settings", "save_sandbox_settings", "read_search_settings", "save_search_settings", "read_backup_settings", "save_backup_settings", "list_backups", "export_backup", "download_backup", "upload_backup", "restore_backup_file", "restore_backup", "read_python_settings", "save_python_settings", "decide", "read_local_model_settings", "save_local_model_settings", "read_local_models", "refresh_local_model", "register_local_model", "install_local_model", "delete_local_model", "cancel_local_model", "list_shells", "save_terminal_settings", "save_power_settings", "list_media_models", "read_media_settings", "save_media_settings", "search_models", "read_model_versions", "resolve_model", "query_knowledge", "read_knowledge_citation", "list_knowledge_collections", "create_knowledge_collection", "update_knowledge_collection", "delete_knowledge_collection", "rebuild_knowledge_collection", "pause_knowledge_collection", "create_terminal", "open_terminal", "open_tool_terminal", "list_terminal_tools", "close_terminal", "list_terminals", "inspect_terminal", "claim_terminal", "input_terminal", "resize_terminal", "set_terminal_appearance", "snapshot", "inspect_host", "read_host_metrics", "use_media", "read_computer_permissions", "request_computer_permission", "stop_host_process", "inspect_request", "inspect_git", "read_git_head", "list_git_branches", "resolve_git_revision", "list_worktrees", "register_worktree", "create_worktree", "create_managed_worktree", "remove_worktree", "create_git_branch", "rename_git_branch", "delete_git_branch", "switch_git_branch", "merge_git_branch", "read_git_stash", "read_git_output", "list_git_remote_tags", "read_git_log", "read_git_commit", "list_git_changes", "read_git_change", "read_git_context", "read_git_show", "read_git_blame", "search_files", "run_git_action", "create_git_commit", "update_git_index", "read_git_diff", "list_directory", "read_file", "search_code", "outline_code", "graph_code", "cancel_code_index_query", "read_code_index_settings", "read_hooks", "save_hooks", "save_code_index_settings", "read_file_lines", "read_image", "find_files", "read_office", "read_notebook", "edit_notebook", "download_file", "upload_file", "upload_attachment", "finish_attachment_upload", "read_attachment", "download_attachment", "download_image", "discard_attachment", "finish_file_upload", "cancel_file_transfer", "write_file", "edit_file", "delete_file", "move_file", "apply_changes", "create_directory", "rename_entry", "copy_entry", "copy_entry_to", "move_entry_to", "trash_entry", "trash_file", "list_credentials", "put_credential", "revoke_credential", "register_project", "create_project", "update_project", "remove_project", "set_defaults", "bind_connection_session", "create_session", "start_session", "read_project_catalog", "read_worktree_catalog", "create_session_at", "import_session", "rename_session", "set_session_read", "set_session_archived", "archive_sessions", "remove_sessions", "set_session_order", "remove_session", "move_conversation", "fork_conversation_at", "set_session_config", "set_session_roles", "set_session_team", "set_session_member_model", "queue_turn"}, "apply_git_hunk")
}

type Request struct {
	Version uint16         `json:"version"`
	ID      RequestID      `json:"id"`
	Target  NodeID         `json:"target"`
	Command Command        `json:"command"`
	Plugin  *PluginContext `json:"plugin,omitempty"`
}

func DefaultRequest() Request {
	return Request{
		Version: 0,
		ID:      RequestID{},
		Target:  NodeID{},
		Command: Command{},
		Plugin:  nil,
	}
}
func (value *Request) UnmarshalJSON(data []byte) error {
	type plain Request
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"version", "id", "target", "command"}, false); err != nil {
		return err
	}
	*value = Request(decoded)
	return nil
}
func (value Request) MarshalJSON() ([]byte, error) {
	type plain Request
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Snapshot struct {
	Node                       NodeID                      `json:"node"`
	Cursor                     uint64                      `json:"cursor"`
	Defaults                   Defaults                    `json:"defaults"`
	Projects                   []Project                   `json:"projects"`
	Worktrees                  []Worktree                  `json:"worktrees"`
	Sessions                   []Session                   `json:"sessions"`
	Terminals                  []TerminalInfo              `json:"terminals"`
	TerminalSettingsRevision   uint64                      `json:"terminal_settings_revision"`
	PowerSettingsRevision      uint64                      `json:"power_settings_revision"`
	SandboxSettingsRevision    uint64                      `json:"sandbox_settings_revision"`
	SearchSettingsRevision     uint64                      `json:"search_settings_revision"`
	MediaSettings              MediaSettings               `json:"media_settings"`
	Providers                  []ConversationProvider      `json:"providers"`
	ProviderQuotas             []ConversationQuotaSnapshot `json:"provider_quotas"`
	ModelCatalog               ConversationCatalogStatus   `json:"model_catalog"`
	Roles                      []RoleProfile               `json:"roles"`
	Teams                      []TeamProfile               `json:"teams"`
	TeamSettingsRevision       uint64                      `json:"team_settings_revision"`
	CompactionSettingsRevision uint64                      `json:"compaction_settings_revision"`
	Ssh                        []SshProfile                `json:"ssh"`
	Databases                  []DatabaseProfile           `json:"databases"`
	Plugins                    []PluginSummary             `json:"plugins"`
	Notifications              []NotificationNotice        `json:"notifications"`
	Turns                      []TurnSummary               `json:"turns"`
}

func DefaultSnapshot() Snapshot {
	return Snapshot{
		Node:                       NodeID{},
		Cursor:                     0,
		Defaults:                   DefaultDefaults(),
		Projects:                   []Project{},
		Worktrees:                  []Worktree{},
		Sessions:                   []Session{},
		Terminals:                  []TerminalInfo{},
		TerminalSettingsRevision:   0,
		PowerSettingsRevision:      0,
		SandboxSettingsRevision:    0,
		SearchSettingsRevision:     0,
		MediaSettings:              DefaultMediaSettings(),
		Providers:                  []ConversationProvider{},
		ProviderQuotas:             []ConversationQuotaSnapshot{},
		ModelCatalog:               DefaultConversationCatalogStatus(),
		Roles:                      []RoleProfile{},
		Teams:                      []TeamProfile{},
		TeamSettingsRevision:       0,
		CompactionSettingsRevision: 0,
		Ssh:                        []SshProfile{},
		Databases:                  []DatabaseProfile{},
		Plugins:                    []PluginSummary{},
		Notifications:              []NotificationNotice{},
		Turns:                      []TurnSummary{},
	}
}
func (value *Snapshot) UnmarshalJSON(data []byte) error {
	type plain Snapshot
	decoded := plain{}
	decoded.Teams = []TeamProfile{}
	decoded.TeamSettingsRevision = 0
	decoded.CompactionSettingsRevision = 0
	if err := DecodeObject(data, &decoded, []string{"node", "cursor", "defaults", "projects", "worktrees", "sessions", "terminals", "terminal_settings_revision", "power_settings_revision", "sandbox_settings_revision", "search_settings_revision", "media_settings", "providers", "provider_quotas", "model_catalog", "roles", "ssh", "databases", "plugins", "notifications", "turns"}, false); err != nil {
		return err
	}
	*value = Snapshot(decoded)
	return nil
}
func (value Snapshot) MarshalJSON() ([]byte, error) {
	type plain Snapshot
	encoded := plain(value)
	encoded.Projects = normalizeCollections(encoded.Projects)
	encoded.Worktrees = normalizeCollections(encoded.Worktrees)
	encoded.Sessions = normalizeCollections(encoded.Sessions)
	encoded.Terminals = normalizeCollections(encoded.Terminals)
	encoded.Providers = normalizeCollections(encoded.Providers)
	encoded.ProviderQuotas = normalizeCollections(encoded.ProviderQuotas)
	encoded.Roles = normalizeCollections(encoded.Roles)
	encoded.Teams = normalizeCollections(encoded.Teams)
	encoded.Ssh = normalizeCollections(encoded.Ssh)
	encoded.Databases = normalizeCollections(encoded.Databases)
	encoded.Plugins = normalizeCollections(encoded.Plugins)
	encoded.Notifications = normalizeCollections(encoded.Notifications)
	encoded.Turns = normalizeCollections(encoded.Turns)
	return json.Marshal(encoded)
}

type RetainedRecord struct {
	Kind string          `json:"kind"`
	Data json.RawMessage `json:"data"`
}

func DefaultRetainedRecord() RetainedRecord {
	return RetainedRecord{
		Kind: "",
		Data: json.RawMessage("null"),
	}
}
func (value *RetainedRecord) UnmarshalJSON(data []byte) error {
	type plain RetainedRecord
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "data"}, true); err != nil {
		return err
	}
	*value = RetainedRecord(decoded)
	return nil
}
func (value RetainedRecord) MarshalJSON() ([]byte, error) {
	type plain RetainedRecord
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Output struct {
	Kind string
	Data any
}
type OutputInstallationDismissedPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputInstallationDismissedPayload() OutputInstallationDismissedPayload {
	return OutputInstallationDismissedPayload{
		Request: RequestID{},
	}
}
func (value *OutputInstallationDismissedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputInstallationDismissedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputInstallationDismissedPayload(decoded)
	return nil
}
func (value OutputInstallationDismissedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputInstallationDismissedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputArtifactWrittenPayload struct {
	Artifact ArtifactSummary `json:"artifact"`
	Warnings []string        `json:"warnings"`
}

func DefaultOutputArtifactWrittenPayload() OutputArtifactWrittenPayload {
	return OutputArtifactWrittenPayload{
		Artifact: DefaultArtifactSummary(),
		Warnings: []string{},
	}
}
func (value *OutputArtifactWrittenPayload) UnmarshalJSON(data []byte) error {
	type plain OutputArtifactWrittenPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"artifact", "warnings"}, false); err != nil {
		return err
	}
	*value = OutputArtifactWrittenPayload(decoded)
	return nil
}
func (value OutputArtifactWrittenPayload) MarshalJSON() ([]byte, error) {
	type plain OutputArtifactWrittenPayload
	encoded := plain(value)
	encoded.Warnings = normalizeCollections(encoded.Warnings)
	return json.Marshal(encoded)
}

type OutputKnowledgeCollectionDeletedPayload struct {
	ID KnowledgeID `json:"id"`
}

func DefaultOutputKnowledgeCollectionDeletedPayload() OutputKnowledgeCollectionDeletedPayload {
	return OutputKnowledgeCollectionDeletedPayload{
		ID: KnowledgeID{},
	}
}
func (value *OutputKnowledgeCollectionDeletedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputKnowledgeCollectionDeletedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = OutputKnowledgeCollectionDeletedPayload(decoded)
	return nil
}
func (value OutputKnowledgeCollectionDeletedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputKnowledgeCollectionDeletedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputCodeIndexQueryCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputCodeIndexQueryCancelledPayload() OutputCodeIndexQueryCancelledPayload {
	return OutputCodeIndexQueryCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputCodeIndexQueryCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputCodeIndexQueryCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputCodeIndexQueryCancelledPayload(decoded)
	return nil
}
func (value OutputCodeIndexQueryCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputCodeIndexQueryCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputCodeUninstalledPayload struct {
	Package string `json:"package"`
	Version string `json:"version"`
}

func DefaultOutputCodeUninstalledPayload() OutputCodeUninstalledPayload {
	return OutputCodeUninstalledPayload{
		Package: "",
		Version: "",
	}
}
func (value *OutputCodeUninstalledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputCodeUninstalledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "version"}, false); err != nil {
		return err
	}
	*value = OutputCodeUninstalledPayload(decoded)
	return nil
}
func (value OutputCodeUninstalledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputCodeUninstalledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputCodeQueryCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputCodeQueryCancelledPayload() OutputCodeQueryCancelledPayload {
	return OutputCodeQueryCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputCodeQueryCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputCodeQueryCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputCodeQueryCancelledPayload(decoded)
	return nil
}
func (value OutputCodeQueryCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputCodeQueryCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputPluginCallStoppingPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputPluginCallStoppingPayload() OutputPluginCallStoppingPayload {
	return OutputPluginCallStoppingPayload{
		Request: RequestID{},
	}
}
func (value *OutputPluginCallStoppingPayload) UnmarshalJSON(data []byte) error {
	type plain OutputPluginCallStoppingPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputPluginCallStoppingPayload(decoded)
	return nil
}
func (value OutputPluginCallStoppingPayload) MarshalJSON() ([]byte, error) {
	type plain OutputPluginCallStoppingPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputWebSearchCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputWebSearchCancelledPayload() OutputWebSearchCancelledPayload {
	return OutputWebSearchCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputWebSearchCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputWebSearchCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputWebSearchCancelledPayload(decoded)
	return nil
}
func (value OutputWebSearchCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputWebSearchCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputWebFetchCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputWebFetchCancelledPayload() OutputWebFetchCancelledPayload {
	return OutputWebFetchCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputWebFetchCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputWebFetchCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputWebFetchCancelledPayload(decoded)
	return nil
}
func (value OutputWebFetchCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputWebFetchCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputPortStreamPayload struct {
	Stream StreamID `json:"stream"`
}

func DefaultOutputPortStreamPayload() OutputPortStreamPayload {
	return OutputPortStreamPayload{
		Stream: StreamID{},
	}
}
func (value *OutputPortStreamPayload) UnmarshalJSON(data []byte) error {
	type plain OutputPortStreamPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream"}, false); err != nil {
		return err
	}
	*value = OutputPortStreamPayload(decoded)
	return nil
}
func (value OutputPortStreamPayload) MarshalJSON() ([]byte, error) {
	type plain OutputPortStreamPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputDatabaseCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputDatabaseCancelledPayload() OutputDatabaseCancelledPayload {
	return OutputDatabaseCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputDatabaseCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputDatabaseCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputDatabaseCancelledPayload(decoded)
	return nil
}
func (value OutputDatabaseCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputDatabaseCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputWslHostInstallCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputWslHostInstallCancelledPayload() OutputWslHostInstallCancelledPayload {
	return OutputWslHostInstallCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputWslHostInstallCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputWslHostInstallCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputWslHostInstallCancelledPayload(decoded)
	return nil
}
func (value OutputWslHostInstallCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputWslHostInstallCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputSshCancelledPayload struct {
	Request RequestID `json:"request"`
}

func DefaultOutputSshCancelledPayload() OutputSshCancelledPayload {
	return OutputSshCancelledPayload{
		Request: RequestID{},
	}
}
func (value *OutputSshCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputSshCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = OutputSshCancelledPayload(decoded)
	return nil
}
func (value OutputSshCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputSshCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputCommandInputAcceptedPayload struct {
	ID    RequestID `json:"id"`
	Bytes uint64    `json:"bytes"`
}

func DefaultOutputCommandInputAcceptedPayload() OutputCommandInputAcceptedPayload {
	return OutputCommandInputAcceptedPayload{
		ID:    RequestID{},
		Bytes: 0,
	}
}
func (value *OutputCommandInputAcceptedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputCommandInputAcceptedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "bytes"}, false); err != nil {
		return err
	}
	*value = OutputCommandInputAcceptedPayload(decoded)
	return nil
}
func (value OutputCommandInputAcceptedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputCommandInputAcceptedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputTurnReplacedPayload struct {
	Turn    QueuedTurn          `json:"turn"`
	History *ConversationRewind `json:"history"`
}

func DefaultOutputTurnReplacedPayload() OutputTurnReplacedPayload {
	return OutputTurnReplacedPayload{
		Turn:    DefaultQueuedTurn(),
		History: nil,
	}
}
func (value *OutputTurnReplacedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputTurnReplacedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "history"}, false); err != nil {
		return err
	}
	*value = OutputTurnReplacedPayload(decoded)
	return nil
}
func (value OutputTurnReplacedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputTurnReplacedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputBackupRestoredPayload struct {
	Record BackupRecord `json:"record"`
	Target string       `json:"target"`
}

func DefaultOutputBackupRestoredPayload() OutputBackupRestoredPayload {
	return OutputBackupRestoredPayload{
		Record: DefaultBackupRecord(),
		Target: "",
	}
}
func (value *OutputBackupRestoredPayload) UnmarshalJSON(data []byte) error {
	type plain OutputBackupRestoredPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"record", "target"}, false); err != nil {
		return err
	}
	*value = OutputBackupRestoredPayload(decoded)
	return nil
}
func (value OutputBackupRestoredPayload) MarshalJSON() ([]byte, error) {
	type plain OutputBackupRestoredPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputLocalModelPayload struct {
	ID string `json:"id"`
}

func DefaultOutputLocalModelPayload() OutputLocalModelPayload {
	return OutputLocalModelPayload{
		ID: "",
	}
}
func (value *OutputLocalModelPayload) UnmarshalJSON(data []byte) error {
	type plain OutputLocalModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = OutputLocalModelPayload(decoded)
	return nil
}
func (value OutputLocalModelPayload) MarshalJSON() ([]byte, error) {
	type plain OutputLocalModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputTerminalInputPayload struct {
	Terminal TerminalID `json:"terminal"`
}

func DefaultOutputTerminalInputPayload() OutputTerminalInputPayload {
	return OutputTerminalInputPayload{
		Terminal: TerminalID{},
	}
}
func (value *OutputTerminalInputPayload) UnmarshalJSON(data []byte) error {
	type plain OutputTerminalInputPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"terminal"}, false); err != nil {
		return err
	}
	*value = OutputTerminalInputPayload(decoded)
	return nil
}
func (value OutputTerminalInputPayload) MarshalJSON() ([]byte, error) {
	type plain OutputTerminalInputPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputRequestOutcomePayload struct {
	ID      RequestID      `json:"id"`
	Outcome RequestOutcome `json:"outcome"`
}

func DefaultOutputRequestOutcomePayload() OutputRequestOutcomePayload {
	return OutputRequestOutcomePayload{
		ID:      RequestID{},
		Outcome: RequestOutcome{},
	}
}
func (value *OutputRequestOutcomePayload) UnmarshalJSON(data []byte) error {
	type plain OutputRequestOutcomePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "outcome"}, false); err != nil {
		return err
	}
	*value = OutputRequestOutcomePayload(decoded)
	return nil
}
func (value OutputRequestOutcomePayload) MarshalJSON() ([]byte, error) {
	type plain OutputRequestOutcomePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputHostProcessSignalledPayload struct {
	Pid           uint32 `json:"pid"`
	StartedAtSecs uint64 `json:"started_at_secs"`
}

func DefaultOutputHostProcessSignalledPayload() OutputHostProcessSignalledPayload {
	return OutputHostProcessSignalledPayload{
		Pid:           0,
		StartedAtSecs: 0,
	}
}
func (value *OutputHostProcessSignalledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputHostProcessSignalledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"pid", "started_at_secs"}, false); err != nil {
		return err
	}
	*value = OutputHostProcessSignalledPayload(decoded)
	return nil
}
func (value OutputHostProcessSignalledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputHostProcessSignalledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputAttachmentDiscardedPayload struct {
	Attachment AttachmentID `json:"attachment"`
}

func DefaultOutputAttachmentDiscardedPayload() OutputAttachmentDiscardedPayload {
	return OutputAttachmentDiscardedPayload{
		Attachment: AttachmentID{},
	}
}
func (value *OutputAttachmentDiscardedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputAttachmentDiscardedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attachment"}, false); err != nil {
		return err
	}
	*value = OutputAttachmentDiscardedPayload(decoded)
	return nil
}
func (value OutputAttachmentDiscardedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputAttachmentDiscardedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputFileTransferCancelledPayload struct {
	Stream StreamID `json:"stream"`
}

func DefaultOutputFileTransferCancelledPayload() OutputFileTransferCancelledPayload {
	return OutputFileTransferCancelledPayload{
		Stream: StreamID{},
	}
}
func (value *OutputFileTransferCancelledPayload) UnmarshalJSON(data []byte) error {
	type plain OutputFileTransferCancelledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream"}, false); err != nil {
		return err
	}
	*value = OutputFileTransferCancelledPayload(decoded)
	return nil
}
func (value OutputFileTransferCancelledPayload) MarshalJSON() ([]byte, error) {
	type plain OutputFileTransferCancelledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputDirectoryCreatedPayload struct {
	Path string `json:"path"`
}

func DefaultOutputDirectoryCreatedPayload() OutputDirectoryCreatedPayload {
	return OutputDirectoryCreatedPayload{
		Path: "",
	}
}
func (value *OutputDirectoryCreatedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputDirectoryCreatedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = OutputDirectoryCreatedPayload(decoded)
	return nil
}
func (value OutputDirectoryCreatedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputDirectoryCreatedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputEntryRenamedPayload struct {
	From string `json:"from"`
	To   string `json:"to"`
}

func DefaultOutputEntryRenamedPayload() OutputEntryRenamedPayload {
	return OutputEntryRenamedPayload{
		From: "",
		To:   "",
	}
}
func (value *OutputEntryRenamedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputEntryRenamedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"from", "to"}, false); err != nil {
		return err
	}
	*value = OutputEntryRenamedPayload(decoded)
	return nil
}
func (value OutputEntryRenamedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputEntryRenamedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputEntryCopiedPayload struct {
	From string `json:"from"`
	To   string `json:"to"`
}

func DefaultOutputEntryCopiedPayload() OutputEntryCopiedPayload {
	return OutputEntryCopiedPayload{
		From: "",
		To:   "",
	}
}
func (value *OutputEntryCopiedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputEntryCopiedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"from", "to"}, false); err != nil {
		return err
	}
	*value = OutputEntryCopiedPayload(decoded)
	return nil
}
func (value OutputEntryCopiedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputEntryCopiedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputEntryMovedPayload struct {
	From string `json:"from"`
	To   string `json:"to"`
}

func DefaultOutputEntryMovedPayload() OutputEntryMovedPayload {
	return OutputEntryMovedPayload{
		From: "",
		To:   "",
	}
}
func (value *OutputEntryMovedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputEntryMovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"from", "to"}, false); err != nil {
		return err
	}
	*value = OutputEntryMovedPayload(decoded)
	return nil
}
func (value OutputEntryMovedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputEntryMovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputEntryTrashedPayload struct {
	Path string `json:"path"`
}

func DefaultOutputEntryTrashedPayload() OutputEntryTrashedPayload {
	return OutputEntryTrashedPayload{
		Path: "",
	}
}
func (value *OutputEntryTrashedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputEntryTrashedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = OutputEntryTrashedPayload(decoded)
	return nil
}
func (value OutputEntryTrashedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputEntryTrashedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputGitRevisionPayload struct {
	Commit string `json:"commit"`
}

func DefaultOutputGitRevisionPayload() OutputGitRevisionPayload {
	return OutputGitRevisionPayload{
		Commit: "",
	}
}
func (value *OutputGitRevisionPayload) UnmarshalJSON(data []byte) error {
	type plain OutputGitRevisionPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"commit"}, false); err != nil {
		return err
	}
	*value = OutputGitRevisionPayload(decoded)
	return nil
}
func (value OutputGitRevisionPayload) MarshalJSON() ([]byte, error) {
	type plain OutputGitRevisionPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputGitBranchDeletedPayload struct {
	Name string `json:"name"`
}

func DefaultOutputGitBranchDeletedPayload() OutputGitBranchDeletedPayload {
	return OutputGitBranchDeletedPayload{
		Name: "",
	}
}
func (value *OutputGitBranchDeletedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputGitBranchDeletedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = OutputGitBranchDeletedPayload(decoded)
	return nil
}
func (value OutputGitBranchDeletedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputGitBranchDeletedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputGitOutputPayload struct {
	Text string `json:"text"`
}

func DefaultOutputGitOutputPayload() OutputGitOutputPayload {
	return OutputGitOutputPayload{
		Text: "",
	}
}
func (value *OutputGitOutputPayload) UnmarshalJSON(data []byte) error {
	type plain OutputGitOutputPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, false); err != nil {
		return err
	}
	*value = OutputGitOutputPayload(decoded)
	return nil
}
func (value OutputGitOutputPayload) MarshalJSON() ([]byte, error) {
	type plain OutputGitOutputPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputGitCommitCreatedPayload struct {
	ID       string `json:"id"`
	FollowUp *Fault `json:"follow_up"`
}

func DefaultOutputGitCommitCreatedPayload() OutputGitCommitCreatedPayload {
	return OutputGitCommitCreatedPayload{
		ID:       "",
		FollowUp: nil,
	}
}
func (value *OutputGitCommitCreatedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputGitCommitCreatedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = OutputGitCommitCreatedPayload(decoded)
	return nil
}
func (value OutputGitCommitCreatedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputGitCommitCreatedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputProjectRemovedPayload struct {
	ID ProjectID `json:"id"`
}

func DefaultOutputProjectRemovedPayload() OutputProjectRemovedPayload {
	return OutputProjectRemovedPayload{
		ID: ProjectID{},
	}
}
func (value *OutputProjectRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputProjectRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = OutputProjectRemovedPayload(decoded)
	return nil
}
func (value OutputProjectRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputProjectRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type OutputWorktreeRemovedPayload struct {
	ID WorktreeID `json:"id"`
}

func DefaultOutputWorktreeRemovedPayload() OutputWorktreeRemovedPayload {
	return OutputWorktreeRemovedPayload{
		ID: WorktreeID{},
	}
}
func (value *OutputWorktreeRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain OutputWorktreeRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = OutputWorktreeRemovedPayload(decoded)
	return nil
}
func (value OutputWorktreeRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain OutputWorktreeRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var outputVariants = variantFactories{"retained_record": func() any { return new(RetainedRecord) },
	"installation_dismissed":       func() any { return new(OutputInstallationDismissedPayload) },
	"installations":                func() any { return new([]InstallationTask) },
	"artifact":                     func() any { return new(ArtifactContent) },
	"artifact_written":             func() any { return new(OutputArtifactWrittenPayload) },
	"artifacts":                    func() any { return new(ArtifactPage) },
	"artifact_revisions":           func() any { return new(ArtifactRevisionPage) },
	"model_search":                 func() any { return new(ModelsPage) },
	"model_versions":               func() any { return new([]ModelsVersion) },
	"model_definition":             func() any { return new(ModelsDefinition) },
	"knowledge_search":             func() any { return new(KnowledgePage) },
	"knowledge_citation":           func() any { return new(KnowledgeCitationPage) },
	"knowledge_collections":        func() any { return new([]KnowledgeCollection) },
	"knowledge_collection":         func() any { return new(KnowledgeCollection) },
	"knowledge_collection_deleted": func() any { return new(OutputKnowledgeCollectionDeletedPayload) },
	"code_search":                  func() any { return new(CodeIndexSearchPage) },
	"code_outline":                 func() any { return new(CodeIndexOutline) },
	"code_graph":                   func() any { return new(CodeIndexGraphGraphPage) },
	"code_index_settings":          func() any { return new(CodeIndexSettings) },
	"code_index_query_cancelled":   func() any { return new(OutputCodeIndexQueryCancelledPayload) },
	"code":                         func() any { return new(CodeResult) },
	"hook_settings":                func() any { return new(HooksState) },
	"code_settings":                func() any { return new(CodeSettings) },
	"code_server":                  func() any { return new(CodeServer) },
	"code_servers":                 func() any { return new([]CodeServer) },
	"code_packages":                func() any { return new(CodeWorkspace) },
	"code_catalog":                 func() any { return new([]CodeLanguage) },
	"code_installation":            func() any { return new(CodeInstallation) },
	"code_installations":           func() any { return new([]CodeInstallation) },
	"code_uninstalled":             func() any { return new(OutputCodeUninstalledPayload) },
	"code_rules":                   func() any { return new(CodeRules) },
	"code_query_cancelled":         func() any { return new(OutputCodeQueryCancelledPayload) },
	"activity_catalog":             func() any { return new(ActivityCatalog) },
	"project_catalog":              func() any { return new(ProjectsCatalog) },
	"plugin_transaction":           func() any { return new([]Output) },
	"plugin_result":                func() any { return new(json.RawMessage) },
	"plugin_call_stopping":         func() any { return new(OutputPluginCallStoppingPayload) },
	"notification":                 func() any { return new(NotificationNotice) },
	"notifications_dismissed":      nil,
	"notifications_updated":        nil,
	"dispatch":                     func() any { return new(DispatchOutput) },
	"browser_completed":            nil,
	"browser":                      func() any { return new(json.RawMessage) },
	"external_browser":             func() any { return new(json.RawMessage) },
	"plugin_value":                 func() any { return new(PluginStorageEntry) },
	"plugin_conversation_value":    func() any { return new(PluginStorageConversationEntry) },
	"plugin_http":                  func() any { return new(PluginHttpResponse) },
	"web_page":                     func() any { return new(WebPage) },
	"search_page":                  func() any { return new(SearchPage) },
	"web_search_cancelled":         func() any { return new(OutputWebSearchCancelledPayload) },
	"web_fetch_cancelled":          func() any { return new(OutputWebFetchCancelledPayload) },
	"plugin_keys":                  func() any { return new(PluginStoragePage) },
	"plugin_search":                func() any { return new(PluginStorageSearchPage) },
	"plugin_text":                  func() any { return new(PluginText) },
	"plugin_models":                func() any { return new(PluginModelsCatalog) },
	"session_config":               func() any { return new(SessionConfig) },
	"file_listing":                 func() any { return new(FileBrowserListing) },
	"files_managed":                func() any { return new([]FileBrowserOutcome) },
	"port_stream":                  func() any { return new(OutputPortStreamPayload) },
	"port_cancelled":               nil,
	"database_profiles":            func() any { return new([]DatabaseProfile) },
	"database_profile":             func() any { return new(DatabaseProfile) },
	"database_outcome":             func() any { return new(DatabaseOutcome) },
	"database_cancelled":           func() any { return new(OutputDatabaseCancelledPayload) },
	"ssh_profiles":                 func() any { return new([]SshProfile) },
	"ssh_profile":                  func() any { return new(SshProfile) },
	"host_install_progress":        func() any { return new(SshInstallProgress) },
	"wsl_distributions":            func() any { return new([]WslDistribution) },
	"host_installation":            func() any { return new(HostInstallationOutcome) },
	"wsl_host_install_cancelled":   func() any { return new(OutputWslHostInstallCancelledPayload) },
	"ssh_outcome":                  func() any { return new(SshOutcome) },
	"ssh_upload":                   func() any { return new(SshUpload) },
	"ssh_cancelled":                func() any { return new(OutputSshCancelledPayload) },
	"plugin_connection":            func() any { return new(PluginConnectionStatus) },
	"connector_types":              func() any { return new([]PluginConnectorType) },
	"connector_identity":           func() any { return new(PluginConnectorIdentity) },
	"connections":                  func() any { return new([]PluginConnectorConnection) },
	"connection":                   func() any { return new(PluginConnectorConnection) },
	"connector_login":              func() any { return new(PluginConnectorAttempt) },
	"plugin":                       func() any { return new(PluginInfo) },
	"catalog_details":              func() any { return new(PluginCatalogDetails) },
	"skill_document":               func() any { return new(PluginSkillsDocument) },
	"skill_discovery":              func() any { return new(PluginSkillsDiscovery) },
	"registry_skill_discovery":     func() any { return new(PluginSkillsRegistryDiscovery) },
	"skill_import_discovery":       func() any { return new(PluginSkillsImportDiscovery) },
	"plugin_catalog":               func() any { return new(PluginCatalogPage) },
	"skill_catalog":                func() any { return new(PluginSkillcatalogPage) },
	"plugin_source":                func() any { return new(PluginCatalogSelection) },
	"plugin_update":                func() any { return new(PluginUpdatesReport) },
	"plugin_updates":               func() any { return new(PluginUpdatesBatch) },
	"plugin_repository":            func() any { return new(PluginSkillsSource) },
	"plugin_upload":                func() any { return new(PluginUpload) },
	"plugin_settings":              func() any { return new(PluginSettingsState) },
	"plugin_mcp":                   func() any { return new(PluginMcpState) },
	"plugin_mcp_checked":           func() any { return new(PluginMcpCheck) },
	"plugin_secret":                func() any { return new(*Secret) },
	"plugin_view":                  func() any { return new(PluginDesktopBundle) },
	"plugins":                      func() any { return new([]PluginSummary) },
	"role":                         func() any { return new(RoleProfile) },
	"roles":                        func() any { return new([]RoleProfile) },
	"team":                         func() any { return new(TeamProfile) },
	"teams":                        func() any { return new([]TeamProfile) },
	"compaction_settings":          func() any { return new(ConversationCompactionSettings) },
	"team_settings":                func() any { return new(TeamSettings) },
	"command_result":               func() any { return new(ProcessCompletion) },
	"command_started":              func() any { return new(ProcessInfo) },
	"command_input_accepted":       func() any { return new(OutputCommandInputAcceptedPayload) },
	"commands":                     func() any { return new([]ProcessInfo) },
	"command_output":               func() any { return new(ProcessSnapshot) },
	"command_output_page":          func() any { return new(ProcessOutputOutputPage) },
	"tool_output":                  func() any { return new(ToolOutputOutputPage) },
	"tool_detail":                  func() any { return new(ToolDetailPage) },
	"provider":                     func() any { return new(ConversationProvider) },
	"providers":                    func() any { return new([]ConversationProvider) },
	"provider_key":                 func() any { return new(*Secret) },
	"provider_login":               func() any { return new(ConversationLoginAttempt) },
	"channel_presets":              func() any { return new(ConversationPresetsListing) },
	"catalog_status":               func() any { return new(ConversationCatalogStatus) },
	"model_catalog":                func() any { return new(ConversationCatalogPage) },
	"model_scores":                 func() any { return new(ConversationCatalogScores) },
	"conversation":                 func() any { return new(ConversationHistory) },
	"conversation_runs":            func() any { return new(ConversationRunsPage) },
	"conversation_run":             func() any { return new(ConversationRunsSummary) },
	"usage":                        func() any { return new(UsageReport) },
	"activity":                     func() any { return new([]ActivityPreview) },
	"discovered_models":            func() any { return new(ConversationDiscoveryCatalog) },
	"provider_validation":          func() any { return new(ConversationDiscoveryValidation) },
	"turn_history":                 func() any { return new(ConversationTurnHistory) },
	"conversation_assets":          func() any { return new(ConversationAssetsPage) },
	"conversation_matches":         func() any { return new(ConversationSearchPage) },
	"file_checkpoints":             func() any { return new(ConversationCheckpointPage) },
	"turn_diff":                    func() any { return new(ConversationCheckpointTurnDiff) },
	"turn_hunks":                   func() any { return new(ConversationCheckpointHunkReview) },
	"hunk_applied":                 func() any { return new(ConversationCheckpointHunkApplied) },
	"file_checkpoint":              func() any { return new(ConversationCheckpointContent) },
	"file_restored":                func() any { return new(ConversationCheckpointRestored) },
	"rewound":                      func() any { return new(ConversationRewind) },
	"turn_replaced":                func() any { return new(OutputTurnReplacedPayload) },
	"run":                          func() any { return new(ConversationRun) },
	"approval":                     func() any { return new(ConversationApproval) },
	"question":                     func() any { return new(ConversationQuestion) },
	"plan_accepted":                func() any { return new(ConversationQuestionAcceptedPlan) },
	"queue":                        func() any { return new(ConversationQueueUpdate) },
	"queued_message":               func() any { return new(ConversationQueuedMessage) },
	"terminal":                     func() any { return new(TerminalInfo) },
	"terminal_settings":            func() any { return new(TerminalSettings) },
	"power_settings":               func() any { return new(PowerSettings) },
	"telemetry_settings":           func() any { return new(TelemetrySettings) },
	"diagnostics_sent":             nil,
	"network_settings":             func() any { return new(NetworkSettings) },
	"network_presets":              func() any { return new(NetworkListing) },
	"network_latency":              func() any { return new(NetworkLatency) },
	"sandbox_settings":             func() any { return new(SandboxSettings) },
	"search_settings":              func() any { return new(SearchSettings) },
	"backup_settings":              func() any { return new(BackupSettings) },
	"backups":                      func() any { return new(BackupPage) },
	"backup":                       func() any { return new(BackupRecord) },
	"backup_download":              func() any { return new(BackupDownload) },
	"backup_upload":                func() any { return new(BackupUpload) },
	"backup_restored":              func() any { return new(OutputBackupRestoredPayload) },
	"python_settings":              func() any { return new(PythonInfo) },
	"local_model_settings":         func() any { return new(ModelsLocalSettings) },
	"local_models":                 func() any { return new(ModelsCatalog) },
	"decision":                     func() any { return new(DecisionResponse) },
	"local_model":                  func() any { return new(OutputLocalModelPayload) },
	"shells":                       func() any { return new([]TerminalShell) },
	"terminal_tools":               func() any { return new([]TerminalToolsToolInfo) },
	"media_settings":               func() any { return new(MediaSettings) },
	"media_models":                 func() any { return new([]MediaCandidate) },
	"media":                        func() any { return new(json.RawMessage) },
	"terminals":                    func() any { return new([]TerminalInfo) },
	"terminal_snapshot":            func() any { return new(TerminalSnapshot) },
	"terminal_input":               func() any { return new(OutputTerminalInputPayload) },
	"request_outcome":              func() any { return new(OutputRequestOutcomePayload) },
	"search_results":               func() any { return new(SearchResults) },
	"snapshot":                     func() any { return new(Snapshot) },
	"host_info":                    func() any { return new(HostInfo) },
	"host_metrics":                 func() any { return new(HostMetricsSample) },
	"computer_permissions":         func() any { return new(ComputerPermissions) },
	"host_process_signalled":       func() any { return new(OutputHostProcessSignalledPayload) },
	"directory":                    func() any { return new(Directory) },
	"file_content":                 func() any { return new(FileContent) },
	"file_lines":                   func() any { return new(FileLines) },
	"file_image":                   func() any { return new(FileImage) },
	"file_names":                   func() any { return new(FileNames) },
	"file_download":                func() any { return new(FileDownload) },
	"file_upload":                  func() any { return new(FileUpload) },
	"attachment":                   func() any { return new(Attachment) },
	"attachment_upload":            func() any { return new(AttachmentUpload) },
	"attachment_download":          func() any { return new(AttachmentDownload) },
	"attachment_discarded":         func() any { return new(OutputAttachmentDiscardedPayload) },
	"file_transfer_cancelled":      func() any { return new(OutputFileTransferCancelledPayload) },
	"file_written":                 func() any { return new(FileWritten) },
	"file_changes":                 func() any { return new(FileChanges) },
	"office_content":               func() any { return new(OfficeInspection) },
	"notebook_page":                func() any { return new(NotebookPage) },
	"directory_created":            func() any { return new(OutputDirectoryCreatedPayload) },
	"entry_renamed":                func() any { return new(OutputEntryRenamedPayload) },
	"entry_copied":                 func() any { return new(OutputEntryCopiedPayload) },
	"entry_moved":                  func() any { return new(OutputEntryMovedPayload) },
	"entry_trashed":                func() any { return new(OutputEntryTrashedPayload) },
	"git_status":                   func() any { return new(GitStatus) },
	"git_head":                     func() any { return new(GitHead) },
	"git_branches":                 func() any { return new(GitBranches) },
	"git_revision":                 func() any { return new(OutputGitRevisionPayload) },
	"git_worktrees":                func() any { return new(GitWorktrees) },
	"git_branch_created":           func() any { return new(GitBranch) },
	"git_branch_renamed":           func() any { return new(GitBranch) },
	"git_merged":                   func() any { return new(GitMerge) },
	"git_branch_switched":          func() any { return new(GitBranch) },
	"git_branch_deleted":           func() any { return new(OutputGitBranchDeletedPayload) },
	"git_log":                      func() any { return new(GitLog) },
	"git_commit":                   func() any { return new(GitCommit) },
	"git_file":                     func() any { return new(GitFile) },
	"git_changes":                  func() any { return new(GitChanges) },
	"git_change":                   func() any { return new(GitChange) },
	"git_context":                  func() any { return new(GitContext) },
	"git_blame":                    func() any { return new(GitBlame) },
	"git_action_completed":         nil,
	"git_output":                   func() any { return new(OutputGitOutputPayload) },
	"git_remote_tags":              func() any { return new([]GitTag) },
	"git_commit_created":           func() any { return new(OutputGitCommitCreatedPayload) },
	"git_diff":                     func() any { return new(GitDiff) },
	"git_index":                    func() any { return new(GitIndex) },
	"credential":                   func() any { return new(Credential) },
	"credentials":                  func() any { return new([]Credential) },
	"project":                      func() any { return new(Project) },
	"project_removed":              func() any { return new(OutputProjectRemovedPayload) },
	"worktree":                     func() any { return new(Worktree) },
	"worktree_removed":             func() any { return new(OutputWorktreeRemovedPayload) },
	"defaults":                     func() any { return new(Defaults) },
	"session":                      func() any { return new(Session) },
	"associated_sessions":          func() any { return new(ConversationAssociationAssociatedSessions) },
	"sessions_archived":            func() any { return new([]Session) },
	"sessions_removed":             func() any { return new([]SessionID) },
	"session_order":                func() any { return new([]SessionID) },
	"queued_turn":                  func() any { return new(QueuedTurn) }}

func (value Output) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, outputVariants)
}
func (value *Output) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", outputVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type RequestOutcome struct {
	Kind string
	Data any
}

var requestOutcomeVariants = variantFactories{"not_admitted": nil,
	"admitted":  nil,
	"completed": func() any { return new(Result[Output, Fault]) },
	"unknown":   nil}

func (value RequestOutcome) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, requestOutcomeVariants)
}
func (value *RequestOutcome) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", requestOutcomeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type Event struct {
	Kind string
	Data any
}
type EventLocalModelsChangedPayload struct {
	Models []ModelsLocalModel `json:"models"`
}

func DefaultEventLocalModelsChangedPayload() EventLocalModelsChangedPayload {
	return EventLocalModelsChangedPayload{
		Models: []ModelsLocalModel{},
	}
}
func (value *EventLocalModelsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventLocalModelsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"models"}, false); err != nil {
		return err
	}
	*value = EventLocalModelsChangedPayload(decoded)
	return nil
}
func (value EventLocalModelsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventLocalModelsChangedPayload
	encoded := plain(value)
	encoded.Models = normalizeCollections(encoded.Models)
	return json.Marshal(encoded)
}

type EventLocalModelSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventLocalModelSettingsChangedPayload() EventLocalModelSettingsChangedPayload {
	return EventLocalModelSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventLocalModelSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventLocalModelSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventLocalModelSettingsChangedPayload(decoded)
	return nil
}
func (value EventLocalModelSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventLocalModelSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventTaskStartedPayload struct {
	Session *Session   `json:"session"`
	Turn    QueuedTurn `json:"turn"`
}

func DefaultEventTaskStartedPayload() EventTaskStartedPayload {
	return EventTaskStartedPayload{
		Session: nil,
		Turn:    DefaultQueuedTurn(),
	}
}
func (value *EventTaskStartedPayload) UnmarshalJSON(data []byte) error {
	type plain EventTaskStartedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn"}, false); err != nil {
		return err
	}
	*value = EventTaskStartedPayload(decoded)
	return nil
}
func (value EventTaskStartedPayload) MarshalJSON() ([]byte, error) {
	type plain EventTaskStartedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventNotificationsReadPayload struct {
	IDs []NotificationID `json:"ids"`
}

func DefaultEventNotificationsReadPayload() EventNotificationsReadPayload {
	return EventNotificationsReadPayload{
		IDs: []NotificationID{},
	}
}
func (value *EventNotificationsReadPayload) UnmarshalJSON(data []byte) error {
	type plain EventNotificationsReadPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids"}, false); err != nil {
		return err
	}
	*value = EventNotificationsReadPayload(decoded)
	return nil
}
func (value EventNotificationsReadPayload) MarshalJSON() ([]byte, error) {
	type plain EventNotificationsReadPayload
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

type EventNotificationsDismissedPayload struct {
	IDs []NotificationID `json:"ids"`
}

func DefaultEventNotificationsDismissedPayload() EventNotificationsDismissedPayload {
	return EventNotificationsDismissedPayload{
		IDs: []NotificationID{},
	}
}
func (value *EventNotificationsDismissedPayload) UnmarshalJSON(data []byte) error {
	type plain EventNotificationsDismissedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"ids"}, false); err != nil {
		return err
	}
	*value = EventNotificationsDismissedPayload(decoded)
	return nil
}
func (value EventNotificationsDismissedPayload) MarshalJSON() ([]byte, error) {
	type plain EventNotificationsDismissedPayload
	encoded := plain(value)
	encoded.IDs = normalizeCollections(encoded.IDs)
	return json.Marshal(encoded)
}

type EventDispatchChangedPayload struct {
	Packages []string `json:"packages"`
}

func DefaultEventDispatchChangedPayload() EventDispatchChangedPayload {
	return EventDispatchChangedPayload{
		Packages: []string{},
	}
}
func (value *EventDispatchChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventDispatchChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"packages"}, false); err != nil {
		return err
	}
	*value = EventDispatchChangedPayload(decoded)
	return nil
}
func (value EventDispatchChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventDispatchChangedPayload
	encoded := plain(value)
	encoded.Packages = normalizeCollections(encoded.Packages)
	return json.Marshal(encoded)
}

type EventPowerSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventPowerSettingsChangedPayload() EventPowerSettingsChangedPayload {
	return EventPowerSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventPowerSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventPowerSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventPowerSettingsChangedPayload(decoded)
	return nil
}
func (value EventPowerSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventPowerSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventTelemetrySettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventTelemetrySettingsChangedPayload() EventTelemetrySettingsChangedPayload {
	return EventTelemetrySettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventTelemetrySettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventTelemetrySettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventTelemetrySettingsChangedPayload(decoded)
	return nil
}
func (value EventTelemetrySettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventTelemetrySettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventTerminalSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventTerminalSettingsChangedPayload() EventTerminalSettingsChangedPayload {
	return EventTerminalSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventTerminalSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventTerminalSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventTerminalSettingsChangedPayload(decoded)
	return nil
}
func (value EventTerminalSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventTerminalSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventSandboxSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventSandboxSettingsChangedPayload() EventSandboxSettingsChangedPayload {
	return EventSandboxSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventSandboxSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventSandboxSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventSandboxSettingsChangedPayload(decoded)
	return nil
}
func (value EventSandboxSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventSandboxSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventSearchSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventSearchSettingsChangedPayload() EventSearchSettingsChangedPayload {
	return EventSearchSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventSearchSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventSearchSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventSearchSettingsChangedPayload(decoded)
	return nil
}
func (value EventSearchSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventSearchSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventBackupSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventBackupSettingsChangedPayload() EventBackupSettingsChangedPayload {
	return EventBackupSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventBackupSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventBackupSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventBackupSettingsChangedPayload(decoded)
	return nil
}
func (value EventBackupSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventBackupSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventPythonSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventPythonSettingsChangedPayload() EventPythonSettingsChangedPayload {
	return EventPythonSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventPythonSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventPythonSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventPythonSettingsChangedPayload(decoded)
	return nil
}
func (value EventPythonSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventPythonSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventPluginValuesChangedPayload struct {
	Name string `json:"name"`
}

func DefaultEventPluginValuesChangedPayload() EventPluginValuesChangedPayload {
	return EventPluginValuesChangedPayload{
		Name: "",
	}
}
func (value *EventPluginValuesChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventPluginValuesChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = EventPluginValuesChangedPayload(decoded)
	return nil
}
func (value EventPluginValuesChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventPluginValuesChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventPluginRemovedPayload struct {
	Name string `json:"name"`
}

func DefaultEventPluginRemovedPayload() EventPluginRemovedPayload {
	return EventPluginRemovedPayload{
		Name: "",
	}
}
func (value *EventPluginRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventPluginRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = EventPluginRemovedPayload(decoded)
	return nil
}
func (value EventPluginRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventPluginRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventSshRemovedPayload struct {
	ID SshID `json:"id"`
}

func DefaultEventSshRemovedPayload() EventSshRemovedPayload {
	return EventSshRemovedPayload{
		ID: SshID{},
	}
}
func (value *EventSshRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventSshRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventSshRemovedPayload(decoded)
	return nil
}
func (value EventSshRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventSshRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventDatabaseRemovedPayload struct {
	ID DatabaseID `json:"id"`
}

func DefaultEventDatabaseRemovedPayload() EventDatabaseRemovedPayload {
	return EventDatabaseRemovedPayload{
		ID: DatabaseID{},
	}
}
func (value *EventDatabaseRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventDatabaseRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventDatabaseRemovedPayload(decoded)
	return nil
}
func (value EventDatabaseRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventDatabaseRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventRoleRemovedPayload struct {
	ID    RoleID        `json:"id"`
	Teams []TeamProfile `json:"teams,omitempty"`
}

func DefaultEventRoleRemovedPayload() EventRoleRemovedPayload {
	return EventRoleRemovedPayload{
		ID:    RoleID{},
		Teams: []TeamProfile{},
	}
}
func (value *EventRoleRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventRoleRemovedPayload
	decoded := plain{}
	decoded.Teams = []TeamProfile{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventRoleRemovedPayload(decoded)
	return nil
}
func (value EventRoleRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventRoleRemovedPayload
	encoded := plain(value)
	encoded.Teams = normalizeCollections(encoded.Teams)
	return json.Marshal(encoded)
}

type EventTeamRemovedPayload struct {
	ID TeamID `json:"id"`
}

func DefaultEventTeamRemovedPayload() EventTeamRemovedPayload {
	return EventTeamRemovedPayload{
		ID: TeamID{},
	}
}
func (value *EventTeamRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventTeamRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventTeamRemovedPayload(decoded)
	return nil
}
func (value EventTeamRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventTeamRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventCompactionSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventCompactionSettingsChangedPayload() EventCompactionSettingsChangedPayload {
	return EventCompactionSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventCompactionSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventCompactionSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventCompactionSettingsChangedPayload(decoded)
	return nil
}
func (value EventCompactionSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventCompactionSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventTeamSettingsChangedPayload struct {
	Revision uint64 `json:"revision"`
}

func DefaultEventTeamSettingsChangedPayload() EventTeamSettingsChangedPayload {
	return EventTeamSettingsChangedPayload{
		Revision: 0,
	}
}
func (value *EventTeamSettingsChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventTeamSettingsChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision"}, false); err != nil {
		return err
	}
	*value = EventTeamSettingsChangedPayload(decoded)
	return nil
}
func (value EventTeamSettingsChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventTeamSettingsChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventCredentialChangedPayload struct {
	Credential CredentialRef `json:"credential"`
	Revision   uint64        `json:"revision"`
}

func DefaultEventCredentialChangedPayload() EventCredentialChangedPayload {
	return EventCredentialChangedPayload{
		Credential: DefaultCredentialRef(),
		Revision:   0,
	}
}
func (value *EventCredentialChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventCredentialChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"credential", "revision"}, false); err != nil {
		return err
	}
	*value = EventCredentialChangedPayload(decoded)
	return nil
}
func (value EventCredentialChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventCredentialChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventProviderQuotaChangedPayload struct {
	Provider ProviderID                 `json:"provider"`
	Quota    *ConversationQuotaSnapshot `json:"quota"`
}

func DefaultEventProviderQuotaChangedPayload() EventProviderQuotaChangedPayload {
	return EventProviderQuotaChangedPayload{
		Provider: ProviderID{},
		Quota:    nil,
	}
}
func (value *EventProviderQuotaChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventProviderQuotaChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider"}, false); err != nil {
		return err
	}
	*value = EventProviderQuotaChangedPayload(decoded)
	return nil
}
func (value EventProviderQuotaChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventProviderQuotaChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventProviderRemovedPayload struct {
	ID ProviderID `json:"id"`
}

func DefaultEventProviderRemovedPayload() EventProviderRemovedPayload {
	return EventProviderRemovedPayload{
		ID: ProviderID{},
	}
}
func (value *EventProviderRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventProviderRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventProviderRemovedPayload(decoded)
	return nil
}
func (value EventProviderRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventProviderRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventConversationChangedPayload struct {
	Session  SessionID        `json:"session"`
	Activity *ActivitySummary `json:"activity"`
}

func DefaultEventConversationChangedPayload() EventConversationChangedPayload {
	return EventConversationChangedPayload{
		Session:  SessionID{},
		Activity: nil,
	}
}
func (value *EventConversationChangedPayload) UnmarshalJSON(data []byte) error {
	type plain EventConversationChangedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "activity"}, false); err != nil {
		return err
	}
	*value = EventConversationChangedPayload(decoded)
	return nil
}
func (value EventConversationChangedPayload) MarshalJSON() ([]byte, error) {
	type plain EventConversationChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventToolsCompletedPayload struct {
	Session SessionID `json:"session"`
}

func DefaultEventToolsCompletedPayload() EventToolsCompletedPayload {
	return EventToolsCompletedPayload{
		Session: SessionID{},
	}
}
func (value *EventToolsCompletedPayload) UnmarshalJSON(data []byte) error {
	type plain EventToolsCompletedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = EventToolsCompletedPayload(decoded)
	return nil
}
func (value EventToolsCompletedPayload) MarshalJSON() ([]byte, error) {
	type plain EventToolsCompletedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventProjectRemovedPayload struct {
	ID ProjectID `json:"id"`
}

func DefaultEventProjectRemovedPayload() EventProjectRemovedPayload {
	return EventProjectRemovedPayload{
		ID: ProjectID{},
	}
}
func (value *EventProjectRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventProjectRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventProjectRemovedPayload(decoded)
	return nil
}
func (value EventProjectRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventProjectRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventWorktreeRemovedPayload struct {
	ID WorktreeID `json:"id"`
}

func DefaultEventWorktreeRemovedPayload() EventWorktreeRemovedPayload {
	return EventWorktreeRemovedPayload{
		ID: WorktreeID{},
	}
}
func (value *EventWorktreeRemovedPayload) UnmarshalJSON(data []byte) error {
	type plain EventWorktreeRemovedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, false); err != nil {
		return err
	}
	*value = EventWorktreeRemovedPayload(decoded)
	return nil
}
func (value EventWorktreeRemovedPayload) MarshalJSON() ([]byte, error) {
	type plain EventWorktreeRemovedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventSessionRewoundPayload struct {
	Session *Session  `json:"session"`
	Backup  **Session `json:"backup,omitempty"`
}

func DefaultEventSessionRewoundPayload() EventSessionRewoundPayload {
	return EventSessionRewoundPayload{
		Session: nil,
		Backup:  nil,
	}
}
func (value *EventSessionRewoundPayload) UnmarshalJSON(data []byte) error {
	type plain EventSessionRewoundPayload
	decoded := plain{}
	decoded.Backup = nil
	if err := DecodeObject(data, &decoded, []string{"session"}, false); err != nil {
		return err
	}
	*value = EventSessionRewoundPayload(decoded)
	return nil
}
func (value EventSessionRewoundPayload) MarshalJSON() ([]byte, error) {
	type plain EventSessionRewoundPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type EventTurnQueuedPayload struct {
	Turn     QueuedTurn       `json:"turn"`
	Activity *ActivitySummary `json:"activity"`
}

func DefaultEventTurnQueuedPayload() EventTurnQueuedPayload {
	return EventTurnQueuedPayload{
		Turn:     DefaultQueuedTurn(),
		Activity: nil,
	}
}
func (value *EventTurnQueuedPayload) UnmarshalJSON(data []byte) error {
	type plain EventTurnQueuedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "activity"}, false); err != nil {
		return err
	}
	*value = EventTurnQueuedPayload(decoded)
	return nil
}
func (value EventTurnQueuedPayload) MarshalJSON() ([]byte, error) {
	type plain EventTurnQueuedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var eventVariants = variantFactories{"retained_record": func() any { return new(RetainedRecord) },
	"local_models_changed":          func() any { return new(EventLocalModelsChangedPayload) },
	"local_model_settings_changed":  func() any { return new(EventLocalModelSettingsChangedPayload) },
	"knowledge_collections_changed": nil,
	"task_started":                  func() any { return new(EventTaskStartedPayload) },
	"notification_changed":          func() any { return new(NotificationNotice) },
	"notifications_read":            func() any { return new(EventNotificationsReadPayload) },
	"notifications_dismissed":       func() any { return new(EventNotificationsDismissedPayload) },
	"dispatch_changed":              func() any { return new(EventDispatchChangedPayload) },
	"media_settings_changed":        func() any { return new(MediaSettings) },
	"power_settings_changed":        func() any { return new(EventPowerSettingsChangedPayload) },
	"telemetry_settings_changed":    func() any { return new(EventTelemetrySettingsChangedPayload) },
	"terminal_settings_changed":     func() any { return new(EventTerminalSettingsChangedPayload) },
	"sandbox_settings_changed":      func() any { return new(EventSandboxSettingsChangedPayload) },
	"search_settings_changed":       func() any { return new(EventSearchSettingsChangedPayload) },
	"backup_settings_changed":       func() any { return new(EventBackupSettingsChangedPayload) },
	"python_settings_changed":       func() any { return new(EventPythonSettingsChangedPayload) },
	"plugin_changed":                func() any { return new(PluginSummary) },
	"plugin_values_changed":         func() any { return new(EventPluginValuesChangedPayload) },
	"plugin_removed":                func() any { return new(EventPluginRemovedPayload) },
	"role_changed":                  func() any { return new(RoleProfile) },
	"ssh_changed":                   func() any { return new(SshProfile) },
	"ssh_removed":                   func() any { return new(EventSshRemovedPayload) },
	"database_changed":              func() any { return new(DatabaseProfile) },
	"database_removed":              func() any { return new(EventDatabaseRemovedPayload) },
	"role_removed":                  func() any { return new(EventRoleRemovedPayload) },
	"team_changed":                  func() any { return new(TeamProfile) },
	"team_removed":                  func() any { return new(EventTeamRemovedPayload) },
	"compaction_settings_changed":   func() any { return new(EventCompactionSettingsChangedPayload) },
	"team_settings_changed":         func() any { return new(EventTeamSettingsChangedPayload) },
	"provider_changed":              func() any { return new(ConversationProvider) },
	"credential_changed":            func() any { return new(EventCredentialChangedPayload) },
	"provider_quota_changed":        func() any { return new(EventProviderQuotaChangedPayload) },
	"model_catalog_changed":         func() any { return new(ConversationCatalogStatus) },
	"provider_removed":              func() any { return new(EventProviderRemovedPayload) },
	"conversation_changed":          func() any { return new(EventConversationChangedPayload) },
	"tools_completed":               func() any { return new(EventToolsCompletedPayload) },
	"terminal_changed":              func() any { return new(TerminalInfo) },
	"project_registered":            func() any { return new(Project) },
	"project_changed":               func() any { return new(Project) },
	"project_removed":               func() any { return new(EventProjectRemovedPayload) },
	"worktree_registered":           func() any { return new(Worktree) },
	"worktree_removed":              func() any { return new(EventWorktreeRemovedPayload) },
	"defaults_changed":              func() any { return new(Defaults) },
	"session_changed":               func() any { return new(Session) },
	"sessions_removed":              func() any { return new([]SessionID) },
	"sessions_reordered":            func() any { return new([]SessionID) },
	"session_rewound":               func() any { return new(EventSessionRewoundPayload) },
	"turn_queued":                   func() any { return new(EventTurnQueuedPayload) },
	"plan_accepted":                 func() any { return new(ConversationQuestionAcceptedPlan) }}

func (value Event) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, eventVariants)
}
func (value *Event) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", eventVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type EventEnvelope struct {
	Node   NodeID `json:"node"`
	Cursor uint64 `json:"cursor"`
	Event  Event  `json:"event"`
}

func DefaultEventEnvelope() EventEnvelope {
	return EventEnvelope{
		Node:   NodeID{},
		Cursor: 0,
		Event:  Event{},
	}
}
func (value *EventEnvelope) UnmarshalJSON(data []byte) error {
	type plain EventEnvelope
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "cursor", "event"}, false); err != nil {
		return err
	}
	*value = EventEnvelope(decoded)
	return nil
}
func (value EventEnvelope) MarshalJSON() ([]byte, error) {
	type plain EventEnvelope
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Update struct {
	Kind string
	Data any
}
type UpdateInstallationsPayload struct {
	Node  NodeID             `json:"node"`
	Tasks []InstallationTask `json:"tasks"`
}

func DefaultUpdateInstallationsPayload() UpdateInstallationsPayload {
	return UpdateInstallationsPayload{
		Node:  NodeID{},
		Tasks: []InstallationTask{},
	}
}
func (value *UpdateInstallationsPayload) UnmarshalJSON(data []byte) error {
	type plain UpdateInstallationsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "tasks"}, false); err != nil {
		return err
	}
	*value = UpdateInstallationsPayload(decoded)
	return nil
}
func (value UpdateInstallationsPayload) MarshalJSON() ([]byte, error) {
	type plain UpdateInstallationsPayload
	encoded := plain(value)
	encoded.Tasks = normalizeCollections(encoded.Tasks)
	return json.Marshal(encoded)
}

type UpdatePythonPayload struct {
	Node         NodeID              `json:"node"`
	Preparations []PythonPreparation `json:"preparations"`
}

func DefaultUpdatePythonPayload() UpdatePythonPayload {
	return UpdatePythonPayload{
		Node:         NodeID{},
		Preparations: []PythonPreparation{},
	}
}
func (value *UpdatePythonPayload) UnmarshalJSON(data []byte) error {
	type plain UpdatePythonPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "preparations"}, false); err != nil {
		return err
	}
	*value = UpdatePythonPayload(decoded)
	return nil
}
func (value UpdatePythonPayload) MarshalJSON() ([]byte, error) {
	type plain UpdatePythonPayload
	encoded := plain(value)
	encoded.Preparations = normalizeCollections(encoded.Preparations)
	return json.Marshal(encoded)
}

type UpdateCommandsPayload struct {
	Node    NodeID        `json:"node"`
	Session SessionID     `json:"session"`
	Items   []ProcessInfo `json:"items"`
}

func DefaultUpdateCommandsPayload() UpdateCommandsPayload {
	return UpdateCommandsPayload{
		Node:    NodeID{},
		Session: SessionID{},
		Items:   []ProcessInfo{},
	}
}
func (value *UpdateCommandsPayload) UnmarshalJSON(data []byte) error {
	type plain UpdateCommandsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "session", "items"}, false); err != nil {
		return err
	}
	*value = UpdateCommandsPayload(decoded)
	return nil
}
func (value UpdateCommandsPayload) MarshalJSON() ([]byte, error) {
	type plain UpdateCommandsPayload
	encoded := plain(value)
	encoded.Items = normalizeCollections(encoded.Items)
	return json.Marshal(encoded)
}

type UpdateFilesChangedPayload struct {
	Node       NodeID     `json:"node"`
	Worktree   WorktreeID `json:"worktree"`
	References uint64     `json:"references"`
}

func DefaultUpdateFilesChangedPayload() UpdateFilesChangedPayload {
	return UpdateFilesChangedPayload{
		Node:       NodeID{},
		Worktree:   WorktreeID{},
		References: 0,
	}
}
func (value *UpdateFilesChangedPayload) UnmarshalJSON(data []byte) error {
	type plain UpdateFilesChangedPayload
	decoded := plain{}
	decoded.References = 0
	if err := DecodeObject(data, &decoded, []string{"node", "worktree"}, false); err != nil {
		return err
	}
	*value = UpdateFilesChangedPayload(decoded)
	return nil
}
func (value UpdateFilesChangedPayload) MarshalJSON() ([]byte, error) {
	type plain UpdateFilesChangedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var updateVariants = variantFactories{"installations": func() any { return new(UpdateInstallationsPayload) },
	"python":                func() any { return new(UpdatePythonPayload) },
	"commands":              func() any { return new(UpdateCommandsPayload) },
	"status":                func() any { return new(PluginUiStatusSnapshot) },
	"browser_call":          func() any { return new(BrowserCall) },
	"connector_login":       func() any { return new(PluginConnectorLoginUpdate) },
	"connections_changed":   func() any { return new(PluginConnectorChanges) },
	"provider_login":        func() any { return new(ConversationLoginUpdate) },
	"conversation_snapshot": func() any { return new(ConversationSnapshot) },
	"conversation_frame":    func() any { return new(ConversationFrame) },
	"terminal_snapshot":     func() any { return new(TerminalSnapshot) },
	"terminal_frame":        func() any { return new(TerminalFrame) },
	"files_changed":         func() any { return new(UpdateFilesChangedPayload) },
	"snapshot":              func() any { return new(Snapshot) },
	"event":                 func() any { return new(EventEnvelope) },
	"reset_required":        nil}

func (value Update) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, updateVariants)
}
func (value *Update) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", updateVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type Topic struct {
	Kind string
	Data any
}

var topicVariants = variantFactories{"installations": nil,
	"scoped_installations": func() any { return new(Request) },
	"python":               nil,
	"commands":             func() any { return new(SessionID) },
	"status":               func() any { return new(PluginUiStatusScope) },
	"browser":              nil,
	"connector_login":      func() any { return new(RequestID) },
	"connections":          nil,
	"provider_login":       func() any { return new(RequestID) },
	"conversation":         func() any { return new(SessionID) },
	"terminal":             func() any { return new(TerminalID) },
	"node":                 nil,
	"files":                func() any { return new(WorktreeID) },
	"repository":           func() any { return new(WorktreeID) }}

func (value Topic) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, topicVariants)
}
func (value *Topic) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", topicVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type Receipt struct {
	ID      RequestID `json:"id"`
	Durable bool      `json:"durable"`
}

func DefaultReceipt() Receipt {
	return Receipt{
		ID:      RequestID{},
		Durable: false,
	}
}
func (value *Receipt) UnmarshalJSON(data []byte) error {
	type plain Receipt
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "durable"}, false); err != nil {
		return err
	}
	*value = Receipt(decoded)
	return nil
}
func (value Receipt) MarshalJSON() ([]byte, error) {
	type plain Receipt
	encoded := plain(value)
	return json.Marshal(encoded)
}
