// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type FileBrowserLocationKind string

const (
	FileBrowserLocationKindHome      FileBrowserLocationKind = "home"
	FileBrowserLocationKindDesktop   FileBrowserLocationKind = "desktop"
	FileBrowserLocationKindDocuments FileBrowserLocationKind = "documents"
	FileBrowserLocationKindDownloads FileBrowserLocationKind = "downloads"
	FileBrowserLocationKindVolume    FileBrowserLocationKind = "volume"
	FileBrowserLocationKindRoot      FileBrowserLocationKind = "root"
)

func (value FileBrowserLocationKind) MarshalJSON() ([]byte, error) {
	switch value {
	case FileBrowserLocationKindHome, FileBrowserLocationKindDesktop, FileBrowserLocationKindDocuments, FileBrowserLocationKindDownloads, FileBrowserLocationKindVolume, FileBrowserLocationKindRoot:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown FileBrowserLocationKind %q", value)
	}
}
func (value *FileBrowserLocationKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch FileBrowserLocationKind(decoded) {
	case FileBrowserLocationKindHome, FileBrowserLocationKindDesktop, FileBrowserLocationKindDocuments, FileBrowserLocationKindDownloads, FileBrowserLocationKindVolume, FileBrowserLocationKindRoot:
		*value = FileBrowserLocationKind(decoded)
		return nil
	default:
		return fmt.Errorf("unknown FileBrowserLocationKind %q", decoded)
	}
}
func (value FileBrowserLocationKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *FileBrowserLocationKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type FileBrowserLocation struct {
	Kind FileBrowserLocationKind `json:"kind"`
	Name string                  `json:"name"`
	Path string                  `json:"path"`
}

func DefaultFileBrowserLocation() FileBrowserLocation {
	return FileBrowserLocation{
		Kind: FileBrowserLocationKind(""),
		Name: "",
		Path: "",
	}
}
func (value *FileBrowserLocation) UnmarshalJSON(data []byte) error {
	type plain FileBrowserLocation
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "name", "path"}, false); err != nil {
		return err
	}
	*value = FileBrowserLocation(decoded)
	return nil
}
func (value FileBrowserLocation) MarshalJSON() ([]byte, error) {
	type plain FileBrowserLocation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileBrowserListing struct {
	Separator Char                  `json:"separator"`
	Locations []FileBrowserLocation `json:"locations"`
	Parent    *string               `json:"parent"`
	Directory Directory             `json:"directory"`
}

func DefaultFileBrowserListing() FileBrowserListing {
	return FileBrowserListing{
		Separator: Char("\u0000"),
		Locations: []FileBrowserLocation{},
		Parent:    nil,
		Directory: DefaultDirectory(),
	}
}
func (value *FileBrowserListing) UnmarshalJSON(data []byte) error {
	type plain FileBrowserListing
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"separator", "locations", "directory"}, false); err != nil {
		return err
	}
	*value = FileBrowserListing(decoded)
	return nil
}
func (value FileBrowserListing) MarshalJSON() ([]byte, error) {
	type plain FileBrowserListing
	encoded := plain(value)
	encoded.Locations = normalizeCollections(encoded.Locations)
	return json.Marshal(encoded)
}

type FileBrowserConflict string

const (
	FileBrowserConflictStop     FileBrowserConflict = "stop"
	FileBrowserConflictReplace  FileBrowserConflict = "replace"
	FileBrowserConflictKeepBoth FileBrowserConflict = "keep_both"
)

func (value FileBrowserConflict) MarshalJSON() ([]byte, error) {
	switch value {
	case FileBrowserConflictStop, FileBrowserConflictReplace, FileBrowserConflictKeepBoth:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown FileBrowserConflict %q", value)
	}
}
func (value *FileBrowserConflict) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch FileBrowserConflict(decoded) {
	case FileBrowserConflictStop, FileBrowserConflictReplace, FileBrowserConflictKeepBoth:
		*value = FileBrowserConflict(decoded)
		return nil
	default:
		return fmt.Errorf("unknown FileBrowserConflict %q", decoded)
	}
}
func (value FileBrowserConflict) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *FileBrowserConflict) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type FileBrowserAction struct {
	Kind string
	Data any
}
type FileBrowserActionCreateDirectoryPayload struct {
	Path string `json:"path"`
}

func DefaultFileBrowserActionCreateDirectoryPayload() FileBrowserActionCreateDirectoryPayload {
	return FileBrowserActionCreateDirectoryPayload{
		Path: "",
	}
}
func (value *FileBrowserActionCreateDirectoryPayload) UnmarshalJSON(data []byte) error {
	type plain FileBrowserActionCreateDirectoryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = FileBrowserActionCreateDirectoryPayload(decoded)
	return nil
}
func (value FileBrowserActionCreateDirectoryPayload) MarshalJSON() ([]byte, error) {
	type plain FileBrowserActionCreateDirectoryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileBrowserActionRenamePayload struct {
	From string `json:"from"`
	To   string `json:"to"`
}

func DefaultFileBrowserActionRenamePayload() FileBrowserActionRenamePayload {
	return FileBrowserActionRenamePayload{
		From: "",
		To:   "",
	}
}
func (value *FileBrowserActionRenamePayload) UnmarshalJSON(data []byte) error {
	type plain FileBrowserActionRenamePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"from", "to"}, false); err != nil {
		return err
	}
	*value = FileBrowserActionRenamePayload(decoded)
	return nil
}
func (value FileBrowserActionRenamePayload) MarshalJSON() ([]byte, error) {
	type plain FileBrowserActionRenamePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileBrowserActionTransferPayload struct {
	Paths       []string            `json:"paths"`
	Destination string              `json:"destination"`
	Cut         bool                `json:"cut"`
	Conflict    FileBrowserConflict `json:"conflict"`
}

func DefaultFileBrowserActionTransferPayload() FileBrowserActionTransferPayload {
	return FileBrowserActionTransferPayload{
		Paths:       []string{},
		Destination: "",
		Cut:         false,
		Conflict:    FileBrowserConflict(""),
	}
}
func (value *FileBrowserActionTransferPayload) UnmarshalJSON(data []byte) error {
	type plain FileBrowserActionTransferPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"paths", "destination", "cut", "conflict"}, false); err != nil {
		return err
	}
	*value = FileBrowserActionTransferPayload(decoded)
	return nil
}
func (value FileBrowserActionTransferPayload) MarshalJSON() ([]byte, error) {
	type plain FileBrowserActionTransferPayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

type FileBrowserActionTrashPayload struct {
	Paths []string `json:"paths"`
}

func DefaultFileBrowserActionTrashPayload() FileBrowserActionTrashPayload {
	return FileBrowserActionTrashPayload{
		Paths: []string{},
	}
}
func (value *FileBrowserActionTrashPayload) UnmarshalJSON(data []byte) error {
	type plain FileBrowserActionTrashPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"paths"}, false); err != nil {
		return err
	}
	*value = FileBrowserActionTrashPayload(decoded)
	return nil
}
func (value FileBrowserActionTrashPayload) MarshalJSON() ([]byte, error) {
	type plain FileBrowserActionTrashPayload
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	return json.Marshal(encoded)
}

var fileBrowserActionVariants = variantFactories{"create_directory": func() any { return new(FileBrowserActionCreateDirectoryPayload) },
	"rename":   func() any { return new(FileBrowserActionRenamePayload) },
	"transfer": func() any { return new(FileBrowserActionTransferPayload) },
	"trash":    func() any { return new(FileBrowserActionTrashPayload) }}

func (value FileBrowserAction) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, fileBrowserActionVariants)
}
func (value *FileBrowserAction) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", fileBrowserActionVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type FileBrowserOutcome struct {
	Path        string  `json:"path"`
	Destination *string `json:"destination"`
	Error       *Fault  `json:"error"`
}

func DefaultFileBrowserOutcome() FileBrowserOutcome {
	return FileBrowserOutcome{
		Path:        "",
		Destination: nil,
		Error:       nil,
	}
}
func (value *FileBrowserOutcome) UnmarshalJSON(data []byte) error {
	type plain FileBrowserOutcome
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = FileBrowserOutcome(decoded)
	return nil
}
func (value FileBrowserOutcome) MarshalJSON() ([]byte, error) {
	type plain FileBrowserOutcome
	encoded := plain(value)
	return json.Marshal(encoded)
}
