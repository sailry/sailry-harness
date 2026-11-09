// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ProcessAccess string

const (
	ProcessAccessWorkspace    ProcessAccess = "workspace"
	ProcessAccessUnrestricted ProcessAccess = "unrestricted"
)

func (value ProcessAccess) MarshalJSON() ([]byte, error) {
	switch value {
	case ProcessAccessWorkspace, ProcessAccessUnrestricted:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ProcessAccess %q", value)
	}
}
func (value *ProcessAccess) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ProcessAccess(decoded) {
	case ProcessAccessWorkspace, ProcessAccessUnrestricted:
		*value = ProcessAccess(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ProcessAccess %q", decoded)
	}
}
func (value ProcessAccess) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ProcessAccess) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ProcessOutcome struct {
	Kind string
	Data any
}

var processOutcomeVariants = variantFactories{"exited": func() any { return new(int32) },
	"signal":    func() any { return new(int32) },
	"timed_out": nil,
	"cancelled": nil,
	"unknown":   func() any { return new(string) }}

func (value ProcessOutcome) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, processOutcomeVariants)
}
func (value *ProcessOutcome) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", processOutcomeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ProcessCapture struct {
	Text        string `json:"text"`
	Truncated   bool   `json:"truncated"`
	InvalidUtf8 bool   `json:"invalid_utf8"`
}

func DefaultProcessCapture() ProcessCapture {
	return ProcessCapture{
		Text:        "",
		Truncated:   false,
		InvalidUtf8: false,
	}
}
func (value *ProcessCapture) UnmarshalJSON(data []byte) error {
	type plain ProcessCapture
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text", "truncated", "invalid_utf8"}, false); err != nil {
		return err
	}
	*value = ProcessCapture(decoded)
	return nil
}
func (value ProcessCapture) MarshalJSON() ([]byte, error) {
	type plain ProcessCapture
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ProcessCompletion struct {
	Outcome   ProcessOutcome `json:"outcome"`
	Stdout    ProcessCapture `json:"stdout"`
	Stderr    ProcessCapture `json:"stderr"`
	ElapsedMs uint64         `json:"elapsed_ms"`
}

func DefaultProcessCompletion() ProcessCompletion {
	return ProcessCompletion{
		Outcome:   ProcessOutcome{},
		Stdout:    DefaultProcessCapture(),
		Stderr:    DefaultProcessCapture(),
		ElapsedMs: 0,
	}
}
func (value *ProcessCompletion) UnmarshalJSON(data []byte) error {
	type plain ProcessCompletion
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"outcome", "stdout", "stderr", "elapsed_ms"}, false); err != nil {
		return err
	}
	*value = ProcessCompletion(decoded)
	return nil
}
func (value ProcessCompletion) MarshalJSON() ([]byte, error) {
	type plain ProcessCompletion
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ProcessStatus struct {
	Kind string
	Data any
}

var processStatusVariants = variantFactories{"running": nil,
	"stopping": nil,
	"finished": func() any { return new(ProcessOutcome) },
	"failed":   func() any { return new(string) }}

func (value ProcessStatus) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, processStatusVariants)
}
func (value *ProcessStatus) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", processStatusVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ProcessInfo struct {
	ID        RequestID        `json:"id"`
	Session   SessionID        `json:"session"`
	Turn      TurnID           `json:"turn"`
	Command   string           `json:"command"`
	Access    ProcessAccess    `json:"access"`
	Pty       bool             `json:"pty"`
	Cwd       string           `json:"cwd"`
	StartedMs uint64           `json:"started_ms"`
	Status    ProcessStatus    `json:"status"`
	Services  []ProcessService `json:"services"`
}

func DefaultProcessInfo() ProcessInfo {
	return ProcessInfo{
		ID:        RequestID{},
		Session:   SessionID{},
		Turn:      TurnID{},
		Command:   "",
		Access:    ProcessAccessWorkspace,
		Pty:       false,
		Cwd:       "",
		StartedMs: 0,
		Status:    ProcessStatus{},
		Services:  []ProcessService{},
	}
}
func (value *ProcessInfo) UnmarshalJSON(data []byte) error {
	type plain ProcessInfo
	decoded := plain{}
	decoded.Pty = false
	if err := DecodeObject(data, &decoded, []string{"id", "session", "turn", "command", "access", "cwd", "started_ms", "status", "services"}, false); err != nil {
		return err
	}
	*value = ProcessInfo(decoded)
	return nil
}
func (value ProcessInfo) MarshalJSON() ([]byte, error) {
	type plain ProcessInfo
	encoded := plain(value)
	encoded.Services = normalizeCollections(encoded.Services)
	return json.Marshal(encoded)
}

type ProcessSnapshot struct {
	Info   ProcessInfo    `json:"info"`
	Stdout ProcessCapture `json:"stdout"`
	Stderr ProcessCapture `json:"stderr"`
}

func DefaultProcessSnapshot() ProcessSnapshot {
	return ProcessSnapshot{
		Info:   DefaultProcessInfo(),
		Stdout: DefaultProcessCapture(),
		Stderr: DefaultProcessCapture(),
	}
}
func (value *ProcessSnapshot) UnmarshalJSON(data []byte) error {
	type plain ProcessSnapshot
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"info", "stdout", "stderr"}, false); err != nil {
		return err
	}
	*value = ProcessSnapshot(decoded)
	return nil
}
func (value ProcessSnapshot) MarshalJSON() ([]byte, error) {
	type plain ProcessSnapshot
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ProcessService struct {
	Port uint16 `json:"port"`
	Url  string `json:"url"`
}

func DefaultProcessService() ProcessService {
	return ProcessService{
		Port: 0,
		Url:  "",
	}
}
func (value *ProcessService) UnmarshalJSON(data []byte) error {
	type plain ProcessService
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"port", "url"}, false); err != nil {
		return err
	}
	*value = ProcessService(decoded)
	return nil
}
func (value ProcessService) MarshalJSON() ([]byte, error) {
	type plain ProcessService
	encoded := plain(value)
	return json.Marshal(encoded)
}
