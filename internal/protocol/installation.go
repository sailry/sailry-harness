// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type InstallationTarget string

const (
	InstallationTargetNode       InstallationTarget = "node"
	InstallationTargetController InstallationTarget = "controller"
)

func (value InstallationTarget) MarshalJSON() ([]byte, error) {
	switch value {
	case InstallationTargetNode, InstallationTargetController:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown InstallationTarget %q", value)
	}
}
func (value *InstallationTarget) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch InstallationTarget(decoded) {
	case InstallationTargetNode, InstallationTargetController:
		*value = InstallationTarget(decoded)
		return nil
	default:
		return fmt.Errorf("unknown InstallationTarget %q", decoded)
	}
}
func (value InstallationTarget) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *InstallationTarget) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type InstallationModelRef struct {
	Kind string
	Data any
}
type InstallationModelRefNodePayload struct {
	Kind ModelsKind `json:"kind"`
	ID   string     `json:"id"`
}

func DefaultInstallationModelRefNodePayload() InstallationModelRefNodePayload {
	return InstallationModelRefNodePayload{
		Kind: ModelsKind(""),
		ID:   "",
	}
}
func (value *InstallationModelRefNodePayload) UnmarshalJSON(data []byte) error {
	type plain InstallationModelRefNodePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "id"}, true); err != nil {
		return err
	}
	*value = InstallationModelRefNodePayload(decoded)
	return nil
}
func (value InstallationModelRefNodePayload) MarshalJSON() ([]byte, error) {
	type plain InstallationModelRefNodePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationModelRefControllerPayload struct {
	Kind ModelsKind `json:"kind"`
	ID   *ModelID   `json:"id"`
}

func DefaultInstallationModelRefControllerPayload() InstallationModelRefControllerPayload {
	return InstallationModelRefControllerPayload{
		Kind: ModelsKind(""),
		ID:   nil,
	}
}
func (value *InstallationModelRefControllerPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationModelRefControllerPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind"}, true); err != nil {
		return err
	}
	*value = InstallationModelRefControllerPayload(decoded)
	return nil
}
func (value InstallationModelRefControllerPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationModelRefControllerPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var installationModelRefVariants = variantFactories{"node": func() any { return new(InstallationModelRefNodePayload) },
	"controller": func() any { return new(InstallationModelRefControllerPayload) }}

func (value InstallationModelRef) MarshalJSON() ([]byte, error) {
	return MarshalUnion("target", "", value.Kind, value.Data, installationModelRefVariants)
}
func (value *InstallationModelRef) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "target", "", installationModelRefVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type InstallationRegistration struct {
	Target     InstallationTarget `json:"target"`
	Definition ModelsDefinition   `json:"definition"`
}

func DefaultInstallationRegistration() InstallationRegistration {
	return InstallationRegistration{
		Target:     InstallationTarget(""),
		Definition: DefaultModelsDefinition(),
	}
}
func (value *InstallationRegistration) UnmarshalJSON(data []byte) error {
	type plain InstallationRegistration
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"target", "definition"}, true); err != nil {
		return err
	}
	*value = InstallationRegistration(decoded)
	return nil
}
func (value InstallationRegistration) MarshalJSON() ([]byte, error) {
	type plain InstallationRegistration
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationResource struct {
	Kind string
	Data any
}
type InstallationResourcePluginPayload struct {
	Name string `json:"name"`
}

func DefaultInstallationResourcePluginPayload() InstallationResourcePluginPayload {
	return InstallationResourcePluginPayload{
		Name: "",
	}
}
func (value *InstallationResourcePluginPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationResourcePluginPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = InstallationResourcePluginPayload(decoded)
	return nil
}
func (value InstallationResourcePluginPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationResourcePluginPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationResourceModelPayload struct {
	Kind ModelsKind `json:"kind"`
	ID   string     `json:"id"`
}

func DefaultInstallationResourceModelPayload() InstallationResourceModelPayload {
	return InstallationResourceModelPayload{
		Kind: ModelsKind(""),
		ID:   "",
	}
}
func (value *InstallationResourceModelPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationResourceModelPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "id"}, true); err != nil {
		return err
	}
	*value = InstallationResourceModelPayload(decoded)
	return nil
}
func (value InstallationResourceModelPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationResourceModelPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationResourceLanguagePayload struct {
	Package string `json:"package"`
	Version string `json:"version"`
}

func DefaultInstallationResourceLanguagePayload() InstallationResourceLanguagePayload {
	return InstallationResourceLanguagePayload{
		Package: "",
		Version: "",
	}
}
func (value *InstallationResourceLanguagePayload) UnmarshalJSON(data []byte) error {
	type plain InstallationResourceLanguagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "version"}, true); err != nil {
		return err
	}
	*value = InstallationResourceLanguagePayload(decoded)
	return nil
}
func (value InstallationResourceLanguagePayload) MarshalJSON() ([]byte, error) {
	type plain InstallationResourceLanguagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var installationResourceVariants = variantFactories{"plugin": func() any { return new(InstallationResourcePluginPayload) },
	"model":    func() any { return new(InstallationResourceModelPayload) },
	"language": func() any { return new(InstallationResourceLanguagePayload) }}

func (value InstallationResource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, installationResourceVariants)
}
func (value *InstallationResource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", installationResourceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type InstallationTask struct {
	Request  RequestID            `json:"request"`
	Revision uint64               `json:"revision"`
	Resource InstallationResource `json:"resource"`
	State    InstallationState    `json:"state"`
}

func DefaultInstallationTask() InstallationTask {
	return InstallationTask{
		Request:  RequestID{},
		Revision: 0,
		Resource: InstallationResource{},
		State:    InstallationState{},
	}
}
func (value *InstallationTask) UnmarshalJSON(data []byte) error {
	type plain InstallationTask
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request", "revision", "resource", "state"}, true); err != nil {
		return err
	}
	*value = InstallationTask(decoded)
	return nil
}
func (value InstallationTask) MarshalJSON() ([]byte, error) {
	type plain InstallationTask
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationState struct {
	Kind string
	Data any
}
type InstallationStateRunningPayload struct {
	Phase    InstallationPhase     `json:"phase"`
	Progress *InstallationProgress `json:"progress"`
}

func DefaultInstallationStateRunningPayload() InstallationStateRunningPayload {
	return InstallationStateRunningPayload{
		Phase:    InstallationPhase(""),
		Progress: nil,
	}
}
func (value *InstallationStateRunningPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationStateRunningPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"phase"}, true); err != nil {
		return err
	}
	*value = InstallationStateRunningPayload(decoded)
	return nil
}
func (value InstallationStateRunningPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationStateRunningPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationStateFailedPayload struct {
	Error Fault `json:"error"`
}

func DefaultInstallationStateFailedPayload() InstallationStateFailedPayload {
	return InstallationStateFailedPayload{
		Error: DefaultFault(),
	}
}
func (value *InstallationStateFailedPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationStateFailedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"error"}, true); err != nil {
		return err
	}
	*value = InstallationStateFailedPayload(decoded)
	return nil
}
func (value InstallationStateFailedPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationStateFailedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationStateUncertainPayload struct {
	Error Fault `json:"error"`
}

func DefaultInstallationStateUncertainPayload() InstallationStateUncertainPayload {
	return InstallationStateUncertainPayload{
		Error: DefaultFault(),
	}
}
func (value *InstallationStateUncertainPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationStateUncertainPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"error"}, true); err != nil {
		return err
	}
	*value = InstallationStateUncertainPayload(decoded)
	return nil
}
func (value InstallationStateUncertainPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationStateUncertainPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var installationStateVariants = variantFactories{"queued": nil,
	"running":   func() any { return new(InstallationStateRunningPayload) },
	"failed":    func() any { return new(InstallationStateFailedPayload) },
	"uncertain": func() any { return new(InstallationStateUncertainPayload) }}

func (value InstallationState) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, installationStateVariants)
}
func (value *InstallationState) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", installationStateVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type InstallationPhase string

const (
	InstallationPhaseDownloading InstallationPhase = "downloading"
	InstallationPhaseInspecting  InstallationPhase = "inspecting"
	InstallationPhasePreparing   InstallationPhase = "preparing"
	InstallationPhaseInstalling  InstallationPhase = "installing"
	InstallationPhaseValidating  InstallationPhase = "validating"
)

func (value InstallationPhase) MarshalJSON() ([]byte, error) {
	switch value {
	case InstallationPhaseDownloading, InstallationPhaseInspecting, InstallationPhasePreparing, InstallationPhaseInstalling, InstallationPhaseValidating:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown InstallationPhase %q", value)
	}
}
func (value *InstallationPhase) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch InstallationPhase(decoded) {
	case InstallationPhaseDownloading, InstallationPhaseInspecting, InstallationPhasePreparing, InstallationPhaseInstalling, InstallationPhaseValidating:
		*value = InstallationPhase(decoded)
		return nil
	default:
		return fmt.Errorf("unknown InstallationPhase %q", decoded)
	}
}
func (value InstallationPhase) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *InstallationPhase) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type InstallationProgress struct {
	Kind string
	Data any
}
type InstallationProgressPercentPayload struct {
	Percent        uint8   `json:"percent"`
	BytesPerSecond *uint64 `json:"bytes_per_second"`
}

func DefaultInstallationProgressPercentPayload() InstallationProgressPercentPayload {
	return InstallationProgressPercentPayload{
		Percent:        0,
		BytesPerSecond: nil,
	}
}
func (value *InstallationProgressPercentPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationProgressPercentPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"percent"}, true); err != nil {
		return err
	}
	*value = InstallationProgressPercentPayload(decoded)
	return nil
}
func (value InstallationProgressPercentPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationProgressPercentPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type InstallationProgressCountsPayload struct {
	Completed      uint64  `json:"completed"`
	Total          uint64  `json:"total"`
	BytesPerSecond *uint64 `json:"bytes_per_second"`
}

func DefaultInstallationProgressCountsPayload() InstallationProgressCountsPayload {
	return InstallationProgressCountsPayload{
		Completed:      0,
		Total:          0,
		BytesPerSecond: nil,
	}
}
func (value *InstallationProgressCountsPayload) UnmarshalJSON(data []byte) error {
	type plain InstallationProgressCountsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"completed", "total"}, true); err != nil {
		return err
	}
	*value = InstallationProgressCountsPayload(decoded)
	return nil
}
func (value InstallationProgressCountsPayload) MarshalJSON() ([]byte, error) {
	type plain InstallationProgressCountsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var installationProgressVariants = variantFactories{"percent": func() any { return new(InstallationProgressPercentPayload) },
	"counts": func() any { return new(InstallationProgressCountsPayload) }}

func (value InstallationProgress) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, installationProgressVariants)
}
func (value *InstallationProgress) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", installationProgressVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
