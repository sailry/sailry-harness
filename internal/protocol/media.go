// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type MediaKind string

const (
	MediaKindVision MediaKind = "vision"
	MediaKindImage  MediaKind = "image"
	MediaKindVideo  MediaKind = "video"
)

func (value MediaKind) MarshalJSON() ([]byte, error) {
	switch value {
	case MediaKindVision, MediaKindImage, MediaKindVideo:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown MediaKind %q", value)
	}
}
func (value *MediaKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch MediaKind(decoded) {
	case MediaKindVision, MediaKindImage, MediaKindVideo:
		*value = MediaKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown MediaKind %q", decoded)
	}
}
func (value MediaKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *MediaKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type MediaGeneration string

const (
	MediaGenerationImage     MediaGeneration = "image"
	MediaGenerationImageTool MediaGeneration = "image_tool"
	MediaGenerationVideo     MediaGeneration = "video"
)

func (value MediaGeneration) MarshalJSON() ([]byte, error) {
	switch value {
	case MediaGenerationImage, MediaGenerationImageTool, MediaGenerationVideo:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown MediaGeneration %q", value)
	}
}
func (value *MediaGeneration) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch MediaGeneration(decoded) {
	case MediaGenerationImage, MediaGenerationImageTool, MediaGenerationVideo:
		*value = MediaGeneration(decoded)
		return nil
	default:
		return fmt.Errorf("unknown MediaGeneration %q", decoded)
	}
}
func (value MediaGeneration) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *MediaGeneration) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type MediaBinding struct {
	Provider ProviderID `json:"provider"`
	Model    string     `json:"model"`
}

func DefaultMediaBinding() MediaBinding {
	return MediaBinding{
		Provider: ProviderID{},
		Model:    "",
	}
}
func (value *MediaBinding) UnmarshalJSON(data []byte) error {
	type plain MediaBinding
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "model"}, false); err != nil {
		return err
	}
	*value = MediaBinding(decoded)
	return nil
}
func (value MediaBinding) MarshalJSON() ([]byte, error) {
	type plain MediaBinding
	encoded := plain(value)
	return json.Marshal(encoded)
}

type MediaSettings struct {
	Revision uint64                      `json:"revision"`
	Bindings map[MediaKind]*MediaBinding `json:"bindings"`
}

func DefaultMediaSettings() MediaSettings {
	return MediaSettings{
		Revision: 0,
		Bindings: map[MediaKind]*MediaBinding{},
	}
}
func (value *MediaSettings) UnmarshalJSON(data []byte) error {
	type plain MediaSettings
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "bindings"}, false); err != nil {
		return err
	}
	*value = MediaSettings(decoded)
	return nil
}
func (value MediaSettings) MarshalJSON() ([]byte, error) {
	type plain MediaSettings
	encoded := plain(value)
	encoded.Bindings = normalizeCollections(encoded.Bindings)
	return json.Marshal(encoded)
}

type MediaSource struct {
	Kind string
	Data any
}

var mediaSourceVariants = variantFactories{"path": func() any { return new(string) },
	"attachment": func() any { return new(AttachmentID) }}

func (value MediaSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "value", value.Kind, value.Data, mediaSourceVariants)
}
func (value *MediaSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "value", mediaSourceVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type MediaAction struct {
	Kind string
	Data any
}
type MediaActionInspectPayload struct {
	Prompt string      `json:"prompt"`
	Source MediaSource `json:"source"`
}

func DefaultMediaActionInspectPayload() MediaActionInspectPayload {
	return MediaActionInspectPayload{
		Prompt: "",
		Source: MediaSource{},
	}
}
func (value *MediaActionInspectPayload) UnmarshalJSON(data []byte) error {
	type plain MediaActionInspectPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"prompt", "source"}, true); err != nil {
		return err
	}
	*value = MediaActionInspectPayload(decoded)
	return nil
}
func (value MediaActionInspectPayload) MarshalJSON() ([]byte, error) {
	type plain MediaActionInspectPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type MediaActionImagePayload struct {
	Prompt string `json:"prompt"`
	Path   string `json:"path"`
}

func DefaultMediaActionImagePayload() MediaActionImagePayload {
	return MediaActionImagePayload{
		Prompt: "",
		Path:   "",
	}
}
func (value *MediaActionImagePayload) UnmarshalJSON(data []byte) error {
	type plain MediaActionImagePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"prompt", "path"}, true); err != nil {
		return err
	}
	*value = MediaActionImagePayload(decoded)
	return nil
}
func (value MediaActionImagePayload) MarshalJSON() ([]byte, error) {
	type plain MediaActionImagePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type MediaActionVideoPayload struct {
	Prompt string `json:"prompt"`
	Path   string `json:"path"`
}

func DefaultMediaActionVideoPayload() MediaActionVideoPayload {
	return MediaActionVideoPayload{
		Prompt: "",
		Path:   "",
	}
}
func (value *MediaActionVideoPayload) UnmarshalJSON(data []byte) error {
	type plain MediaActionVideoPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"prompt", "path"}, true); err != nil {
		return err
	}
	*value = MediaActionVideoPayload(decoded)
	return nil
}
func (value MediaActionVideoPayload) MarshalJSON() ([]byte, error) {
	type plain MediaActionVideoPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var mediaActionVariants = variantFactories{"inspect": func() any { return new(MediaActionInspectPayload) },
	"image": func() any { return new(MediaActionImagePayload) },
	"video": func() any { return new(MediaActionVideoPayload) }}

func (value MediaAction) MarshalJSON() ([]byte, error) {
	return MarshalUnion("action", "", value.Kind, value.Data, mediaActionVariants)
}
func (value *MediaAction) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "action", "", mediaActionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type MediaCandidate struct {
	Provider     ProviderID  `json:"provider"`
	ProviderName string      `json:"provider_name"`
	Model        string      `json:"model"`
	Kinds        []MediaKind `json:"kinds"`
}

func DefaultMediaCandidate() MediaCandidate {
	return MediaCandidate{
		Provider:     ProviderID{},
		ProviderName: "",
		Model:        "",
		Kinds:        []MediaKind{},
	}
}
func (value *MediaCandidate) UnmarshalJSON(data []byte) error {
	type plain MediaCandidate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"provider", "provider_name", "model", "kinds"}, false); err != nil {
		return err
	}
	*value = MediaCandidate(decoded)
	return nil
}
func (value MediaCandidate) MarshalJSON() ([]byte, error) {
	type plain MediaCandidate
	encoded := plain(value)
	encoded.Kinds = normalizeCollections(encoded.Kinds)
	return json.Marshal(encoded)
}
