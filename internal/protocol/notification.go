// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type NotificationKind string

const (
	NotificationKindInfo    NotificationKind = "info"
	NotificationKindSuccess NotificationKind = "success"
	NotificationKindWarning NotificationKind = "warning"
	NotificationKindError   NotificationKind = "error"
)

func (value NotificationKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *NotificationKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch NotificationKind(decoded) {
	case NotificationKindInfo, NotificationKindSuccess, NotificationKindWarning, NotificationKindError:
		*value = NotificationKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value NotificationKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *NotificationKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type NotificationDraft struct {
	Title        string                    `json:"title"`
	Message      string                    `json:"message"`
	Kind         NotificationKind          `json:"kind"`
	Session      *SessionID                `json:"session"`
	Presentation *NotificationPresentation `json:"presentation"`
	Details      *NotificationDetails      `json:"details"`
}

func DefaultNotificationDraft() NotificationDraft {
	return NotificationDraft{
		Title:        "",
		Message:      "",
		Kind:         NotificationKind(""),
		Session:      nil,
		Presentation: nil,
		Details:      nil,
	}
}
func (value *NotificationDraft) UnmarshalJSON(data []byte) error {
	type plain NotificationDraft
	decoded := plain{}
	decoded.Presentation = nil
	decoded.Details = nil
	if err := DecodeObject(data, &decoded, []string{"title", "message", "kind"}, true); err != nil {
		return err
	}
	*value = NotificationDraft(decoded)
	return nil
}
func (value NotificationDraft) MarshalJSON() ([]byte, error) {
	type plain NotificationDraft
	encoded := plain(value)
	return json.Marshal(encoded)
}

type NotificationPresentation struct {
	Heading    string            `json:"heading"`
	Locales    map[string]string `json:"locales"`
	Persistent bool              `json:"persistent"`
}

func DefaultNotificationPresentation() NotificationPresentation {
	return NotificationPresentation{
		Heading:    "",
		Locales:    map[string]string{},
		Persistent: false,
	}
}
func (value *NotificationPresentation) UnmarshalJSON(data []byte) error {
	type plain NotificationPresentation
	decoded := plain{}
	decoded.Locales = map[string]string{}
	decoded.Persistent = false
	if err := DecodeObject(data, &decoded, []string{"heading"}, true); err != nil {
		return err
	}
	*value = NotificationPresentation(decoded)
	return nil
}
func (value NotificationPresentation) MarshalJSON() ([]byte, error) {
	type plain NotificationPresentation
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	return json.Marshal(encoded)
}

type NotificationNotice struct {
	ID          NotificationID    `json:"id"`
	Sequence    uint64            `json:"sequence"`
	Package     string            `json:"package"`
	Content     NotificationDraft `json:"content"`
	TimestampMs int64             `json:"timestamp_ms"`
	Read        bool              `json:"read"`
}

func DefaultNotificationNotice() NotificationNotice {
	return NotificationNotice{
		ID:          NotificationID{},
		Sequence:    0,
		Package:     "",
		Content:     DefaultNotificationDraft(),
		TimestampMs: 0,
		Read:        false,
	}
}
func (value *NotificationNotice) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *NotificationNotice) DecodeJSON(decoder *Decoder) error {
	type plain NotificationNotice
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "sequence", "package", "content", "timestamp_ms", "read"}, true); err != nil {
		return err
	}
	*value = NotificationNotice(decoded)
	return nil
}
func (value NotificationNotice) MarshalJSON() ([]byte, error) {
	type plain NotificationNotice
	encoded := plain(value)
	return json.Marshal(encoded)
}
