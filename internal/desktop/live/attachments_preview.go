package live

import (
	"context"
	"fmt"
	"path/filepath"
	"unicode/utf8"

	htmltomarkdown "github.com/JohannesKaufmann/html-to-markdown/v2"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const attachmentPreviewMaximum = 2 << 20

type attachmentDocumentImage struct {
	image           *conversation.Image
	loading, failed bool
}

func (r *Runtime) openCapturedFilePreview(session string, file conversation.File, descriptor attachmentDescriptor) {
	if descriptor.artifact == nil {
		file.Kind = attachmentFileKind(file.Path, descriptor.mime)
	}
	if file.Kind == conversation.OtherFile {
		r.openExternalAttachment(descriptor, false)
		return
	}
	file.ID = "preview/" + session + "/" + file.ID
	r.attachments.previews[file.ID] = descriptor
	r.attachments.previewGeneration[file.ID]++
	generation := r.attachments.previewGeneration[file.ID]
	file.State = conversation.PreviewLoading
	r.App.Panel.ShowArtifact(file)
	r.App.PanelOpen = true
	r.async(func() {
		loaded, content, err := r.readAttachmentPreview(r.attachments.ctx, descriptor, file)
		r.enqueue(func(c *ui.Context) {
			if r.attachments.previewGeneration[file.ID] != generation {
				return
			}
			if err != nil {
				loaded.State = conversation.PreviewFailed
				if faultCode(err) == protocol.ErrorCodeRevisionConflict || faultCode(err) == protocol.ErrorCodeConflict {
					loaded.State = conversation.PreviewChanged
				}
				r.downloadOutcome(c, err)
			} else if content != nil {
				r.attachments.previewHTML[file.ID] = content.Html
			}
			r.App.Panel.UpdateArtifact(loaded)
		})
	})
}
func (r *Runtime) readAttachmentPreview(ctx context.Context, descriptor attachmentDescriptor, file conversation.File) (conversation.File, *protocol.ArtifactContent, error) {
	file.State = conversation.PreviewReady
	if descriptor.artifact != nil {
		content, err := r.readPublishedArtifact(ctx, descriptor)
		if err != nil {
			return file, nil, err
		}
		file.Kind = conversation.MarkdownFile
		file.Text, err = htmltomarkdown.ConvertString(content.Html)
		return file, &content, err
	}
	if file.Kind == conversation.ImageFile {
		data, err := r.readAttachmentImage(ctx, descriptor)
		if err != nil {
			return file, nil, err
		}
		bitmap, err := decodeAttachmentBitmap(data)
		if err != nil {
			return file, nil, err
		}
		width, height := bitmap.Size()
		file.Image = &conversation.Image{ID: file.ID, Name: file.Name(), Width: width, Height: height, Source: bitmap}
		return file, nil, nil
	}
	stream, size, digest, err := r.prepareAttachmentDownload(ctx, descriptor)
	if err != nil {
		return file, nil, err
	}
	file.Size = int64(size)
	if size > attachmentPreviewMaximum {
		r.cancelAttachmentStream(stream)
		file.State = conversation.PreviewLarge
		return file, nil, nil
	}
	sink := &imageDownloadSink{limit: size}
	if err = r.controller.Download(ctx, stream, size, digest, sink, nil); err != nil {
		return file, nil, err
	}
	if !utf8.Valid(sink.data) {
		return file, nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "file preview is not UTF-8 text")
	}
	file.Text = string(sink.data)
	return file, nil, nil
}
func (r *Runtime) readPublishedArtifact(ctx context.Context, descriptor attachmentDescriptor) (protocol.ArtifactContent, error) {
	artifact := descriptor.artifact
	if artifact == nil {
		return protocol.ArtifactContent{}, fileActionFault(protocol.ErrorCodeNotFound, "document source is unavailable")
	}
	revision := artifact.Revision
	output, err := r.controller.Do(ctx, protocol.Command{Kind: "read_artifact", Data: &protocol.CommandReadArtifactPayload{ID: artifact.ID, Revision: &revision, Session: &descriptor.session}})
	if err != nil {
		return protocol.ArtifactContent{}, err
	}
	content, ok := detail[protocol.ArtifactContent](output.Data)
	if !ok || output.Kind != "artifact" || content.Artifact != *artifact {
		return protocol.ArtifactContent{}, fileActionFault(protocol.ErrorCodeRevisionConflict, "published document revision changed")
	}
	return content, nil
}

// OpenArtifactExternal and ArtifactImageAction receive only captured preview IDs.
func (r *Runtime) OpenArtifactExternal(file conversation.File) {
	r.enqueue(func(c *ui.Context) {
		descriptor, ok := r.attachments.previews[file.ID]
		if !ok {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "file preview source is unavailable"))
			return
		}
		if descriptor.artifact != nil {
			r.openExternalArtifact(descriptor)
			return
		}
		r.openExternalAttachment(descriptor, false)
	})
}
func (r *Runtime) ArtifactImageAction(action kit.ImageAction, image conversation.Image) {
	r.enqueue(func(c *ui.Context) {
		descriptor, ok := r.attachments.previews[image.ID]
		if !ok {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "image preview source is unavailable"))
			return
		}
		r.attachmentImageDescriptorAction(c, action, descriptor)
	})
}
func (r *Runtime) chooseArtifactDestination(descriptor attachmentDescriptor) {
	native := r.attachments.native
	if native.SaveFile == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native save panel is unavailable"))
		return
	}
	native.SaveFile(filepath.Base(descriptor.name), func(path string) {
		if path == "" {
			return
		}
		r.writePublishedArtifact(descriptor, path, nil)
	})
}
func (r *Runtime) writePublishedArtifact(descriptor attachmentDescriptor, path string, finished func(error)) {
	r.async(func() {
		content, err := r.readPublishedArtifact(r.attachments.ctx, descriptor)
		if err == nil {
			target, e := prepareDownloadDestination(path)
			err = e
			if e == nil {
				defer target.close()
				_, err = target.file.WriteString(content.Html)
				if err == nil {
					err = target.publish(r.attachments.ctx)
				}
			}
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.downloadOutcome(c, err)
			}
			if finished != nil {
				finished(err)
			} else if err == nil {
				kit.Toast(c, kit.Done, i18n.T(c, "files_download_done"))
			}
		})
	})
}
func (r *Runtime) openExternalArtifact(descriptor attachmentDescriptor) {
	native := r.attachments.native
	if native.OpenLocal == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native file action is unavailable"))
		return
	}
	r.async(func() {
		directory, release, err := r.ownedDownloadDirectory()
		if err != nil {
			r.Report(err)
			return
		}
		path := filepath.Join(directory, filepath.Base(descriptor.name))
		r.writePublishedArtifact(descriptor, path, func(err error) {
			if err != nil {
				release()
				return
			}
			native.OpenLocal(path)
		})
	})
}
func (r *Runtime) projectDocumentThumbnail(session, id string, descriptor attachmentDescriptor) *conversation.Image {
	artifact := descriptor.artifact
	native := r.attachments.native
	if artifact == nil || artifact.Kind != protocol.ArtifactKindPreview || native.RenderDocument == nil {
		return nil
	}
	key := fmt.Sprintf("%s/%s/%d", session, artifact.ID, artifact.Revision)
	image := r.attachments.documents[key]
	if image == nil {
		if len(r.attachments.documents) >= 48 {
			return nil
		}
		image = &attachmentDocumentImage{}
		r.attachments.documents[key] = image
	}
	if image.image != nil || image.loading || image.failed {
		return image.image
	}
	image.loading = true
	ctx := r.attachmentImages(session).ctx
	r.async(func() {
		content, err := r.readPublishedArtifact(ctx, descriptor)
		r.enqueue(func(*ui.Context) {
			if ctx.Err() != nil {
				return
			}
			if err != nil {
				image.loading, image.failed = false, true
				return
			}
			native.RenderDocument(ctx, *artifact, content.Html, func(data []byte, err error) {
				r.async(func() {
					var bitmap *ui.Bitmap
					if err == nil && len(data) > 8*1024*1024 {
						err = fileActionFault(protocol.ErrorCodeInvalidRequest, "document thumbnail exceeds its byte limit")
					}
					if err == nil {
						bitmap, err = decodeAttachmentBitmap(data)
					}
					r.enqueue(func(*ui.Context) {
						if ctx.Err() != nil || r.attachments.documents[key] != image {
							return
						}
						image.loading, image.failed = false, err != nil
						if err == nil {
							width, height := bitmap.Size()
							image.image = &conversation.Image{ID: id, Name: descriptor.name, Width: width, Height: height, Source: bitmap}
							r.projectConversations()
						}
					})
				})
			})
		})
	})
	return nil
}

func (r *Runtime) renderArtifactPreview(c *ui.Context, file conversation.File) bool {
	if r.attachments.native.DocumentView == nil {
		return false
	}
	descriptor, ok := r.attachments.previews[file.ID]
	html, loaded := r.attachments.previewHTML[file.ID]
	if !ok || !loaded || descriptor.artifact == nil {
		return false
	}
	return r.attachments.native.DocumentView(c, *descriptor.artifact, html)
}
