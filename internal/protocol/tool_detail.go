// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ToolDetailReference struct {
	Session  SessionID `json:"session"`
	Turn     TurnID    `json:"turn"`
	Entry    string    `json:"entry"`
	Index    uint64    `json:"index"`
	Call     *string   `json:"call"`
	Revision string    `json:"revision"`
}

func DefaultToolDetailReference() ToolDetailReference {
	return ToolDetailReference{
		Session:  SessionID{},
		Turn:     TurnID{},
		Entry:    "",
		Index:    0,
		Call:     nil,
		Revision: "",
	}
}
func (value *ToolDetailReference) UnmarshalJSON(data []byte) error {
	type plain ToolDetailReference
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "turn", "entry", "index", "revision"}, false); err != nil {
		return err
	}
	*value = ToolDetailReference(decoded)
	return nil
}
func (value ToolDetailReference) MarshalJSON() ([]byte, error) {
	type plain ToolDetailReference
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDetailSummary struct {
	Revision    string                     `json:"revision"`
	Icon        *PluginDesktopIcon         `json:"icon,omitempty"`
	Resolved    *ToolDetailResolved        `json:"resolved,omitempty"`
	HasDetails  bool                       `json:"has_details"`
	Published   bool                       `json:"published,omitempty"`
	Failure     *ToolFailure               `json:"failure,omitempty"`
	Diagnostics bool                       `json:"diagnostics"`
	Expected    *ToolResultExpected        `json:"expected,omitempty"`
	Error       *string                    `json:"error,omitempty"`
	RawResult   bool                       `json:"raw_result"`
	Images      uint64                     `json:"images"`
	Bytes       *uint64                    `json:"bytes,omitempty"`
	Lines       *uint64                    `json:"lines,omitempty"`
	Path        *string                    `json:"path,omitempty"`
	Title       *string                    `json:"title,omitempty"`
	Resource    *string                    `json:"resource,omitempty"`
	SummaryID   *string                    `json:"summary_id,omitempty"`
	Delegation  ToolDetailDelegation       `json:"delegation"`
	Question    **ConversationQuestionSpec `json:"question,omitempty"`
	Progress    *ConversationProgress      `json:"progress,omitempty"`
	// Task is the id of the background task the call started: a command
	// that was sent to the background. It is absent for any other call.
	Task *string `json:"task,omitempty"`
}

func DefaultToolDetailSummary() ToolDetailSummary {
	return ToolDetailSummary{
		Revision:    "",
		Icon:        nil,
		Resolved:    nil,
		HasDetails:  false,
		Published:   false,
		Failure:     nil,
		Diagnostics: false,
		Expected:    nil,
		Error:       nil,
		RawResult:   false,
		Images:      0,
		Bytes:       nil,
		Lines:       nil,
		Path:        nil,
		Title:       nil,
		Resource:    nil,
		SummaryID:   nil,
		Delegation:  DefaultToolDetailDelegation(),
		Question:    nil,
		Progress:    nil,
		Task:        nil,
	}
}
func (value *ToolDetailSummary) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ToolDetailSummary) DecodeJSON(decoder *Decoder) error {
	type plain ToolDetailSummary
	decoded := plain{}
	decoded.Icon = nil
	decoded.Resolved = nil
	decoded.Published = false
	decoded.Failure = nil
	decoded.Expected = nil
	decoded.Error = nil
	decoded.Bytes = nil
	decoded.Lines = nil
	decoded.Path = nil
	decoded.Title = nil
	decoded.Resource = nil
	decoded.SummaryID = nil
	decoded.Delegation = DefaultToolDetailDelegation()
	decoded.Question = nil
	decoded.Progress = nil
	decoded.Task = nil
	if err := decoder.Object(&decoded, []string{"revision", "has_details", "diagnostics", "raw_result", "images"}, false); err != nil {
		return err
	}
	*value = ToolDetailSummary(decoded)
	return nil
}
func (value ToolDetailSummary) MarshalJSON() ([]byte, error) {
	type plain ToolDetailSummary
	encoded := plain(value)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Delegation.Session == nil && encoded.Delegation.Worktree == nil && !encoded.Delegation.Invalid && !encoded.Delegation.Background {
		delete(fields, "delegation")
	}
	return json.Marshal(fields)
}

type ToolDetailDelegation struct {
	Session    *SessionID  `json:"session,omitempty"`
	Worktree   *WorktreeID `json:"worktree,omitempty"`
	Invalid    bool        `json:"invalid,omitempty"`
	Background bool        `json:"background,omitempty"`
}

func DefaultToolDetailDelegation() ToolDetailDelegation {
	return ToolDetailDelegation{
		Session:    nil,
		Worktree:   nil,
		Invalid:    false,
		Background: false,
	}
}
func (value *ToolDetailDelegation) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ToolDetailDelegation) DecodeJSON(decoder *Decoder) error {
	type plain ToolDetailDelegation
	decoded := plain{}
	decoded.Session = nil
	decoded.Worktree = nil
	decoded.Invalid = false
	decoded.Background = false
	if err := decoder.Object(&decoded, []string{}, false); err != nil {
		return err
	}
	*value = ToolDetailDelegation(decoded)
	return nil
}
func (value ToolDetailDelegation) MarshalJSON() ([]byte, error) {
	type plain ToolDetailDelegation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDetailResolved struct {
	Label       *ToolResultLocalText   `json:"label"`
	Input       ToolDetailInput        `json:"input"`
	Output      *ToolDetailOutput      `json:"output"`
	Approval    []ToolResultLocalText  `json:"approval"`
	Status      *ToolResultLocalText   `json:"status"`
	Command     *ProcessOutcome        `json:"command,omitempty"`
	Diagnostics []ToolResultDiagnostic `json:"diagnostics"`
}

func DefaultToolDetailResolved() ToolDetailResolved {
	return ToolDetailResolved{
		Label:       nil,
		Input:       DefaultToolDetailInput(),
		Output:      nil,
		Approval:    []ToolResultLocalText{},
		Status:      nil,
		Command:     nil,
		Diagnostics: []ToolResultDiagnostic{},
	}
}
func (value *ToolDetailResolved) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ToolDetailResolved) DecodeJSON(decoder *Decoder) error {
	type plain ToolDetailResolved
	decoded := plain{}
	decoded.Command = nil
	if err := decoder.Object(&decoded, []string{"input", "approval", "diagnostics"}, false); err != nil {
		return err
	}
	*value = ToolDetailResolved(decoded)
	return nil
}
func (value ToolDetailResolved) MarshalJSON() ([]byte, error) {
	type plain ToolDetailResolved
	encoded := plain(value)
	encoded.Approval = normalizeCollections(encoded.Approval)
	encoded.Diagnostics = normalizeCollections(encoded.Diagnostics)
	return json.Marshal(encoded)
}

type ToolDetailInput struct {
	Summary *ToolDetailText `json:"summary"`
	Context *ToolDetailText `json:"context"`
	Target  *ToolDetailText `json:"target"`
	Content *ToolDetailText `json:"content"`
}

func DefaultToolDetailInput() ToolDetailInput {
	return ToolDetailInput{
		Summary: nil,
		Context: nil,
		Target:  nil,
		Content: nil,
	}
}
func (value *ToolDetailInput) UnmarshalJSON(data []byte) error {
	type plain ToolDetailInput
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = ToolDetailInput(decoded)
	return nil
}
func (value ToolDetailInput) MarshalJSON() ([]byte, error) {
	type plain ToolDetailInput
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDetailText struct {
	Text string `json:"text"`
	Code bool   `json:"code"`
}

func DefaultToolDetailText() ToolDetailText {
	return ToolDetailText{
		Text: "",
		Code: false,
	}
}
func (value *ToolDetailText) UnmarshalJSON(data []byte) error {
	type plain ToolDetailText
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text", "code"}, false); err != nil {
		return err
	}
	*value = ToolDetailText(decoded)
	return nil
}
func (value ToolDetailText) MarshalJSON() ([]byte, error) {
	type plain ToolDetailText
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDetailOutput struct {
	Preview         string                `json:"preview"`
	PreviewDeferred bool                  `json:"preview_deferred,omitempty"`
	Notices         []ToolResultLocalText `json:"notices"`
	Paths           []string              `json:"paths"`
	PathCount       uint64                `json:"path_count,omitempty"`
	Body            *ToolDetailBody       `json:"body"`
	Table           *ToolContentTable     `json:"table"`
}

func DefaultToolDetailOutput() ToolDetailOutput {
	return ToolDetailOutput{
		Preview:         "",
		PreviewDeferred: false,
		Notices:         []ToolResultLocalText{},
		Paths:           []string{},
		PathCount:       0,
		Body:            nil,
		Table:           nil,
	}
}
func (value *ToolDetailOutput) UnmarshalJSON(data []byte) error {
	type plain ToolDetailOutput
	decoded := plain{}
	decoded.PreviewDeferred = false
	decoded.PathCount = 0
	if err := DecodeObject(data, &decoded, []string{"preview", "notices", "paths"}, false); err != nil {
		return err
	}
	*value = ToolDetailOutput(decoded)
	return nil
}
func (value ToolDetailOutput) MarshalJSON() ([]byte, error) {
	type plain ToolDetailOutput
	encoded := plain(value)
	encoded.Notices = normalizeCollections(encoded.Notices)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type ToolDetailBody struct {
	Text  string                       `json:"text"`
	Path  *string                      `json:"path"`
	Diff  *ToolDisplayProjectionFormat `json:"diff"`
	Empty *ToolResultLocalText         `json:"empty"`
}

func DefaultToolDetailBody() ToolDetailBody {
	return ToolDetailBody{
		Text:  "",
		Path:  nil,
		Diff:  nil,
		Empty: nil,
	}
}
func (value *ToolDetailBody) UnmarshalJSON(data []byte) error {
	type plain ToolDetailBody
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, false); err != nil {
		return err
	}
	*value = ToolDetailBody(decoded)
	return nil
}
func (value ToolDetailBody) MarshalJSON() ([]byte, error) {
	type plain ToolDetailBody
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolDetail struct {
	Arguments json.RawMessage     `json:"arguments"`
	Result    *json.RawMessage    `json:"result"`
	Resolved  *ToolDetailResolved `json:"resolved"`
	Content   *ToolContent        `json:"content"`
	Images    []ConversationImage `json:"images"`
}

func DefaultToolDetail() ToolDetail {
	return ToolDetail{
		Arguments: json.RawMessage("null"),
		Result:    nil,
		Resolved:  nil,
		Content:   nil,
		Images:    []ConversationImage{},
	}
}
func (value *ToolDetail) UnmarshalJSON(data []byte) error {
	type plain ToolDetail
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"arguments", "images"}, false); err != nil {
		return err
	}
	*value = ToolDetail(decoded)
	return nil
}
func (value ToolDetail) MarshalJSON() ([]byte, error) {
	type plain ToolDetail
	encoded := plain(value)
	encoded.Images = normalizeCollections(encoded.Images)
	return json.Marshal(encoded)
}

type ToolDetailPage struct {
	Reference  ToolDetailReference `json:"reference"`
	Offset     uint64              `json:"offset"`
	TotalBytes uint64              `json:"total_bytes"`
	NextOffset *uint64             `json:"next_offset"`
	Text       string              `json:"text"`
}

func DefaultToolDetailPage() ToolDetailPage {
	return ToolDetailPage{
		Reference:  DefaultToolDetailReference(),
		Offset:     0,
		TotalBytes: 0,
		NextOffset: nil,
		Text:       "",
	}
}
func (value *ToolDetailPage) UnmarshalJSON(data []byte) error {
	type plain ToolDetailPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"reference", "offset", "total_bytes", "text"}, false); err != nil {
		return err
	}
	*value = ToolDetailPage(decoded)
	return nil
}
func (value ToolDetailPage) MarshalJSON() ([]byte, error) {
	type plain ToolDetailPage
	encoded := plain(value)
	return json.Marshal(encoded)
}
