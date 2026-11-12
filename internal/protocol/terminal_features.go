// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type TerminalFeatures struct {
	FocusReporting  bool                       `json:"focus_reporting"`
	AlternateScroll bool                       `json:"alternate_scroll"`
	Title           string                     `json:"title"`
	Directory       string                     `json:"directory"`
	Bell            uint64                     `json:"bell"`
	Clipboard       *TerminalFeaturesClipboard `json:"clipboard"`
}

func DefaultTerminalFeatures() TerminalFeatures {
	return TerminalFeatures{
		FocusReporting:  false,
		AlternateScroll: false,
		Title:           "",
		Directory:       "",
		Bell:            0,
		Clipboard:       nil,
	}
}
func (value *TerminalFeatures) UnmarshalJSON(data []byte) error {
	type plain TerminalFeatures
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"focus_reporting", "alternate_scroll", "title", "directory", "bell"}, false); err != nil {
		return err
	}
	*value = TerminalFeatures(decoded)
	return nil
}
func (value TerminalFeatures) MarshalJSON() ([]byte, error) {
	type plain TerminalFeatures
	encoded := plain(value)
	return json.Marshal(encoded)
}

type TerminalFeaturesClipboard struct {
	Sequence uint64 `json:"sequence"`
	Text     string `json:"text"`
}

func DefaultTerminalFeaturesClipboard() TerminalFeaturesClipboard {
	return TerminalFeaturesClipboard{
		Sequence: 0,
		Text:     "",
	}
}
func (value *TerminalFeaturesClipboard) UnmarshalJSON(data []byte) error {
	type plain TerminalFeaturesClipboard
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"sequence", "text"}, false); err != nil {
		return err
	}
	*value = TerminalFeaturesClipboard(decoded)
	return nil
}
func (value TerminalFeaturesClipboard) MarshalJSON() ([]byte, error) {
	type plain TerminalFeaturesClipboard
	encoded := plain(value)
	return json.Marshal(encoded)
}
