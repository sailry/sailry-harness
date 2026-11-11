package engine

import (
	"encoding/hex"
	"encoding/json"
	"github.com/MoeForge/moego/base64"
	"github.com/gabriel-vasile/mimetype"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const attachmentPrefix = "sailry-attachment://"
const attachmentReadNote = "Contents not included; use read_office with source {\"kind\":\"attachment\",\"value\":\"<id>\"} for PDF, Office documents and spreadsheets; use an attachment-capable tool for other files"

type attachmentMaterial struct {
	attachment protocol.Attachment
	current    bool
	data       []byte
	err        error
	loaded     bool
}

// hydrateAttachments changes the request's copy only. Canonical history retains
// stable attachment references, independent of provider representation.
func (r *Runtime) hydrateAttachments(x *execution, records []canonical.Record) ([]canonical.Record, map[string][]byte, error) {
	raw, err := json.Marshal(records)
	if err != nil {
		return nil, nil, err
	}
	var request []canonical.Record
	if err = json.Unmarshal(raw, &request); err != nil {
		return nil, nil, err
	}
	assets := map[string][]byte{}
	remaining := uint64(64 << 20)
	vision := false
	for _, model := range x.inv.Provider.Models {
		if model.ID == x.inv.Turn.Config.Model {
			vision = model.Vision
			break
		}
	}
	if x.attachments == nil {
		x.attachments = map[protocol.AttachmentID]*attachmentMaterial{}
	}
	for i := range request {
		message := request[i].MutableMessage()
		if message == nil {
			continue
		}
		var content []canonical.Block
		changed := false
		for _, block := range message.Content {
			if block.Kind == "image" && !vision {
				changed = true
				continue
			}
			file, ok := block.Data.(*canonical.FileContent)
			if !ok || !strings.HasPrefix(file.File.URI, attachmentPrefix) {
				content = append(content, block)
				continue
			}
			changed = true
			id, err := protocol.ParseAttachmentID(strings.TrimPrefix(file.File.URI, attachmentPrefix))
			if err != nil {
				return nil, nil, err
			}
			if r.attachment == nil || r.attachmentBytes == nil {
				return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "attachment materialization is unavailable"}
			}
			entry := x.attachments[id]
			if entry == nil {
				entry = &attachmentMaterial{}
				entry.attachment, entry.current, entry.err = r.attachment(x.ctx, x.inv.Run.Turn, id)
				x.attachments[id] = entry
			}
			if entry.err != nil {
				if entry.current {
					return nil, nil, entry.err
				}
				mime := ""
				if file.File.Mime != nil {
					mime = *file.File.Mime
				}
				descriptor, _ := json.Marshal(map[string]any{"id": id, "media_type": mime})
				content = append(content, canonical.Text(uint64(len(content)), "Attachment reference: "+string(descriptor)+"\nAttachment unavailable; contents not included"))
				continue
			}
			attachment := entry.attachment
			if file.File.Mime == nil || *file.File.Mime != attachment.Spec.MediaType {
				return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "attachment media type mismatch"}
			}
			included := false
			if attachment.Spec.Size <= remaining {
				if !entry.loaded {
					entry.data, entry.err = r.attachmentBytes(x.ctx, attachment)
					entry.loaded = true
				}
				if entry.err != nil {
					if entry.current {
						return nil, nil, entry.err
					}
					code := nodeAttachmentCode(entry.err)
					if code != protocol.ErrorCodeNotFound && code != protocol.ErrorCodeRevisionConflict {
						return nil, nil, entry.err
					}
					descriptor, _ := json.Marshal(map[string]any{"id": id, "media_type": attachment.Spec.MediaType})
					content = append(content, canonical.Text(uint64(len(content)), "Attachment reference: "+string(descriptor)+"\nAttachment unavailable; contents not included"))
					continue
				}
				mime := strings.Split(mimetype.Detect(entry.data).String(), ";")[0]
				switch {
				case strings.HasPrefix(mime, "image/") && vision:
					digest := blake3.Sum256(entry.data)
					key := hex.EncodeToString(digest[:])
					assets[key] = entry.data
					content = append(content, canonical.Block{Index: uint64(len(content)), Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: key, Mime: mime, Size: uint64(len(entry.data))}}})
					included = true
				case mime == "application/pdf" && attachmentPDFSupported(x.inv.Provider.Api):
					media := mime
					content = append(content, canonical.Block{Index: uint64(len(content)), Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: base64.StdEncoding.EncodeToString(entry.data), Mime: &media}}})
					included = true
				case len(entry.data) <= 1<<20 && utf8.Valid(entry.data) && !strings.ContainsRune(string(entry.data), 0) && (strings.HasPrefix(mime, "text/") || mime == "application/json" || mime == "application/xml" || mime == "application/yaml" || mime == "application/octet-stream"):
					content = append(content, canonical.Text(uint64(len(content)), "Attachment: "+attachment.Spec.Name+"\n"+string(entry.data)))
					included = true
				}
				if included {
					remaining -= attachment.Spec.Size
				}
			}
			descriptor, _ := json.Marshal(map[string]any{"id": id, "name": attachment.Spec.Name, "media_type": attachment.Spec.MediaType, "size": attachment.Spec.Size})
			note := "Attachment reference: " + string(descriptor)
			if !included {
				note += "\n" + attachmentReadNote
			}
			content = append(content, canonical.Text(uint64(len(content)), note))
		}
		if !changed {
			continue
		}
		for index := range content {
			content[index].Index = uint64(index)
		}
		message.Content, message.PartCount = content, uint64(len(content))
	}
	return request, assets, nil
}
func nodeAttachmentCode(err error) protocol.ErrorCode {
	if fault, ok := err.(*protocol.Fault); ok {
		return fault.Code
	}
	return protocol.ErrorCodeInternal
}

func attachmentPDFSupported(api protocol.ConversationModelApi) bool {
	switch api {
	case protocol.ConversationModelApiResponses, protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiAnthropic, protocol.ConversationModelApiGemini, protocol.ConversationModelApiAzureOpenAi:
		return true
	}
	return false
}
