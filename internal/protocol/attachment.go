// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type AttachmentSpec struct {
	Worktree  WorktreeID `json:"worktree"`
	Name      string     `json:"name"`
	MediaType string     `json:"media_type"`
	Size      uint64     `json:"size"`
	Revision  string     `json:"revision"`
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
	type plain AttachmentSpec
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "name", "media_type", "size", "revision"}, false); err != nil {
		return err
	}
	*value = AttachmentSpec(decoded)
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
func (value *Attachment) UnmarshalJSON(data []byte) error {
	type plain Attachment
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "spec"}, false); err != nil {
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
