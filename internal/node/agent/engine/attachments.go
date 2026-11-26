package engine

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/MoeForge/moego/base64"
	"github.com/gabriel-vasile/mimetype"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const attachmentPrefix = "sailry-attachment://"
const attachmentReadNote = "Contents not included; use read_office with source {\"kind\":\"attachment\",\"value\":\"<id>\"} for PDF, Office documents and spreadsheets; use an attachment-capable tool for other files"

type attachmentMaterial struct {
	attachment    protocol.Attachment
	current       bool
	data          []byte
	err           error
	mime          string
	loaded        bool
	prepared      bool
	referenceOnly bool
}

// hydrateAttachments changes the request's copy only. Canonical history retains
// stable attachment references, independent of provider representation. It
// runs every model step, so it copies the record list and then only a message
// that holds an attachment or an image the model cannot see; every other
// record is the caller's, unchanged.
func (r *Runtime) hydrateAttachments(x *execution, records []canonical.Record) ([]canonical.Record, map[string][]byte, error) {
	request := slices.Clone(records)
	assets := map[string][]byte{}
	remaining := uint64(64 << 20)
	binding := model.Binding{Provider: *x.inv.Provider, Config: x.inv.Turn.Config}
	vision := model.ReadsImages(binding)
	inspect := findTool(x.tools, "inspect_image")
	canInspect := inspect != nil && inspect.Binding != nil && inspect.Binding.Owner.Name == "" &&
		inspect.Binding.Operation != nil && *inspect.Binding.Operation == protocol.ToolOperationInspectMedia && capturedModel(binding).Tools
	if x.attachments == nil {
		x.attachments = map[protocol.AttachmentID]*attachmentMaterial{}
	}
	// Only inline content consumes the budget, so retaining bytes for a
	// reference-only attachment could grow past it on a long turn.
	referenced := make(map[protocol.AttachmentID]bool)
	for i := range request {
		message := request[i].MutableMessage()
		if message == nil {
			continue
		}
		var content []canonical.Block
		input := request[i].Metadata.Input
		for index, block := range message.Content {
			drop := block.Kind == "image" && !vision ||
				input != nil && input.CapabilityPart != nil && *input.CapabilityPart == block.Index
			file, ok := block.Data.(*canonical.FileContent)
			if !drop && (!ok || !strings.HasPrefix(file.File.URI, attachmentPrefix)) {
				if content != nil {
					content = append(content, block)
				}
				continue
			}
			if content == nil {
				content = make([]canonical.Block, index, len(message.Content))
				copy(content, message.Content[:index])
			}
			if drop {
				continue
			}
			id, err := protocol.ParseAttachmentID(strings.TrimPrefix(file.File.URI, attachmentPrefix))
			if err != nil {
				return nil, nil, err
			}
			if r.attachment == nil || r.attachmentBytes == nil {
				return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "attachment materialization is unavailable"}
			}
			if _, seen := referenced[id]; !seen {
				referenced[id] = false
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
			declared := strings.ToLower(attachment.Spec.MediaType)
			mime := declared
			if entry.mime != "" {
				mime = entry.mime
			}
			image := strings.HasPrefix(mime, "image/")
			mayInclude := !entry.referenceOnly
			if mayInclude && entry.mime != "" {
				switch {
				case image:
					mayInclude = vision
				case mime == "application/pdf" || attachmentAudioVideo(mime):
					mayInclude = model.AttachmentFileMIME(binding, mime) != ""
				}
			}
			if mayInclude && attachment.Spec.Size <= remaining {
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
				if entry.mime == "" {
					entry.mime, _, _ = strings.Cut(mimetype.Detect(entry.data).String(), ";")
				}
				mime = entry.mime
				image = strings.HasPrefix(mime, "image/")
				switch {
				case image && vision:
					if !entry.prepared {
						source, sent, encoded, err := files.PrepareImage(entry.data)
						if err != nil {
							entry.referenceOnly = true
							break
						}
						mime = source.MimeType
						if sent != nil {
							mime = sent.MimeType
						}
						entry.data, entry.mime, entry.prepared = encoded, mime, true
					}
					if uint64(len(entry.data)) > remaining {
						break
					}
					digest := blake3.Sum256(entry.data)
					key := hex.EncodeToString(digest[:])
					assets[key] = entry.data
					content = append(content, canonical.Block{Index: uint64(len(content)), Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: key, Mime: mime, Size: uint64(len(entry.data))}}})
					included = true
				case mime == "application/pdf" || attachmentAudioVideo(mime):
					if media := model.AttachmentFileMIME(binding, mime); media != "" {
						content = append(content, canonical.Block{Index: uint64(len(content)), Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: base64.StdEncoding.EncodeToString(entry.data), Mime: &media}}})
						included = true
					}
				case len(entry.data) <= 1<<20 && utf8.Valid(entry.data) && bytes.IndexByte(entry.data, 0) < 0 && (strings.HasPrefix(mime, "text/") || mime == "application/json" || mime == "application/xml" || mime == "application/yaml" || mime == "application/octet-stream"):
					content = append(content, canonical.Text(uint64(len(content)), "Attachment: "+attachment.Spec.Name+"\n"+string(entry.data)))
					included = true
				}
				if included {
					if image {
						remaining -= uint64(len(entry.data))
					} else {
						remaining -= attachment.Spec.Size
					}
					referenced[id] = true
				} else {
					entry.data, entry.loaded, entry.prepared = nil, false, false
					// A model change can make detected media eligible again.
					entry.referenceOnly = entry.referenceOnly || !image && mime != "application/pdf" && !attachmentAudioVideo(mime)
				}
			}
			descriptor, _ := json.Marshal(map[string]any{"id": id, "name": attachment.Spec.Name, "media_type": attachment.Spec.MediaType, "size": attachment.Spec.Size})
			note := "Attachment reference: " + string(descriptor)
			if !included {
				switch {
				case !image && (mime == "application/pdf" || declared == "application/pdf"):
					note += "\nThe PDF is not included directly. Use read_office with source {\"kind\":\"attachment\",\"value\":\"" + id.String() + "\"}."
				case !image && (attachmentAudioVideo(mime) || attachmentAudioVideo(declared)):
					note += "\nThe conversation model cannot directly read this attachment in this request; only its reference is included."
				case !image:
					note += "\n" + attachmentReadNote
				case entry.referenceOnly:
					note += "\nThe image could not be prepared for model input; only its reference is included."
				case canInspect && attachment.Spec.Size <= protocol.MAX_IMAGE_BYTES:
					note += "\nImage contents are omitted from this request. Use inspect_image with {\"attachment\":\"" + id.String() + "\",\"prompt\":\"<question about the image>\"}. If needed, load it with tool_search and query \"select:inspect_image\"."
				case !vision:
					note += "\nThe conversation model lacks image input, and image inspection is unavailable for this attachment."
				default:
					note += "\nThe image exceeds the remaining inline attachment budget, and image inspection is unavailable for this attachment."
				}
			}
			content = append(content, canonical.Text(uint64(len(content)), note))
		}
		if content == nil {
			continue
		}
		for index := range content {
			content[index].Index = uint64(index)
		}
		hydrated := *message
		hydrated.Content, hydrated.PartCount = content, uint64(len(content))
		if input := request[i].Metadata.Input; input != nil && input.CapabilityPart != nil {
			copied := *input
			copied.CapabilityPart = nil
			request[i].Metadata.Input = &copied
		}
		if compaction, ok := request[i].Data.(*canonical.Compaction); ok {
			summarized := *compaction
			summarized.Summary = hydrated
			request[i].Data = &summarized
		} else {
			request[i].Data = &hydrated
		}
	}
	for id, entry := range x.attachments {
		included, present := referenced[id]
		if !present {
			delete(x.attachments, id)
		} else if !included && entry.err == nil && !entry.referenceOnly {
			entry.data, entry.loaded, entry.prepared = nil, false, false
		}
	}
	return request, assets, nil
}
func nodeAttachmentCode(err error) protocol.ErrorCode {
	if fault, ok := err.(*protocol.Fault); ok {
		return fault.Code
	}
	return protocol.ErrorCodeInternal
}

func attachmentAudioVideo(mime string) bool {
	return strings.HasPrefix(mime, "audio/") || strings.HasPrefix(mime, "video/")
}
