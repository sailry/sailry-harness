// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type AttachmentSpec struct {
	// A zero Worktree stages a private controller attachment before the first turn.
	Worktree  WorktreeID `json:"worktree"`
	Name      string     `json:"name"`
	MediaType string     `json:"media_type"`
	Size      uint64     `json:"size"`
	Revision  string     `json:"revision"`
}

// attachmentWorktreeID admits the draft scope only on attachment transfers.
type attachmentWorktreeID WorktreeID

func (value *attachmentWorktreeID) DecodeJSON(decoder *Decoder) error {
	text, err := decoder.String()
	if err != nil {
		return err
	}
	if text == (WorktreeID{}).String() {
		*value = attachmentWorktreeID{}
		return nil
	}
	worktree, err := ParseWorktreeID(text)
	if err == nil {
		*value = attachmentWorktreeID(worktree)
	}
	return err
}

type attachmentLocation struct {
	Worktree   attachmentWorktreeID `json:"worktree"`
	Attachment AttachmentID         `json:"attachment"`
}

// UsesDraftAttachment identifies a transfer whose owner is the authenticated
// controller rather than a worktree.
func (command Command) UsesDraftAttachment() bool {
	zero := WorktreeID{}
	switch input := command.Data.(type) {
	case *AttachmentSpec:
		return command.Kind == "upload_attachment" && input != nil && input.Worktree == zero
	case *CommandFinishAttachmentUploadPayload:
		return command.Kind == "finish_attachment_upload" && input != nil && input.Worktree == zero
	case *CommandDownloadAttachmentPayload:
		return command.Kind == "download_attachment" && input != nil && input.Worktree == zero
	case *CommandDiscardAttachmentPayload:
		return command.Kind == "discard_attachment" && input != nil && input.Worktree == zero
	}
	return false
}

func DefaultAttachmentSpec() AttachmentSpec {
	return AttachmentSpec{
		Worktree:  WorktreeID{},
		Name:      "",
		MediaType: "",
		Size:      0,
		Revision:  "",
	}
}
func (value *AttachmentSpec) UnmarshalJSON(data []byte) error {
	var decoded struct {
		Worktree  attachmentWorktreeID `json:"worktree"`
		Name      string               `json:"name"`
		MediaType string               `json:"media_type"`
		Size      uint64               `json:"size"`
		Revision  string               `json:"revision"`
	}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "media_type", "size", "revision"}, false); err != nil {
		return err
	}
	*value = AttachmentSpec{Worktree: WorktreeID(decoded.Worktree), Name: decoded.Name, MediaType: decoded.MediaType, Size: decoded.Size, Revision: decoded.Revision}
	return nil
}
func (value AttachmentSpec) MarshalJSON() ([]byte, error) {
	type plain AttachmentSpec
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Attachment struct {
	ID   AttachmentID   `json:"id"`
	Spec AttachmentSpec `json:"spec"`
}

func DefaultAttachment() Attachment {
	return Attachment{
		ID:   AttachmentID{},
		Spec: DefaultAttachmentSpec(),
	}
}
func (value *Attachment) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *Attachment) DecodeJSON(decoder *Decoder) error {
	type plain Attachment
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "spec"}, false); err != nil {
		return err
	}
	*value = Attachment(decoded)
	return nil
}
func (value Attachment) MarshalJSON() ([]byte, error) {
	type plain Attachment
	encoded := plain(value)
	return json.Marshal(encoded)
}

type AttachmentUpload struct {
	Stream StreamID       `json:"stream"`
	Spec   AttachmentSpec `json:"spec"`
}

func DefaultAttachmentUpload() AttachmentUpload {
	return AttachmentUpload{
		Stream: StreamID{},
		Spec:   DefaultAttachmentSpec(),
	}
}
func (value *AttachmentUpload) UnmarshalJSON(data []byte) error {
	type plain AttachmentUpload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "spec"}, false); err != nil {
		return err
	}
	*value = AttachmentUpload(decoded)
	return nil
}
func (value AttachmentUpload) MarshalJSON() ([]byte, error) {
	type plain AttachmentUpload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type AttachmentDownload struct {
	Stream     StreamID   `json:"stream"`
	Attachment Attachment `json:"attachment"`
}

func DefaultAttachmentDownload() AttachmentDownload {
	return AttachmentDownload{
		Stream:     StreamID{},
		Attachment: DefaultAttachment(),
	}
}
func (value *AttachmentDownload) UnmarshalJSON(data []byte) error {
	type plain AttachmentDownload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "attachment"}, false); err != nil {
		return err
	}
	*value = AttachmentDownload(decoded)
	return nil
}
func (value AttachmentDownload) MarshalJSON() ([]byte, error) {
	type plain AttachmentDownload
	encoded := plain(value)
	return json.Marshal(encoded)
}
