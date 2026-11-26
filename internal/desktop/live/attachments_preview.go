package live

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"
	"unicode/utf8"

	htmltomarkdown "github.com/JohannesKaufmann/html-to-markdown/v2"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const attachmentPreviewMaximum = 2 << 20

type attachmentDocumentImage struct {
	image                    *conversation.Image
	thumbnail                *attachmentThumbnail
	cancel                   context.CancelFunc
	visible, loading, failed bool
}

type documentPreview struct {
	shown, loading *documentImages
}

func (r *Runtime) openCapturedFilePreview(session string, file conversation.File, descriptor attachmentDescriptor) {
	if descriptor.artifact == nil {
		file.Kind = attachmentFileKind(file.Path, descriptor.mime)
	}
	if file.Kind == conversation.OtherFile {
		r.openExternalAttachment(descriptor, false)
		return
	}
	file.ID = "preview/" + r.controller.Target().String() + "/" + session + "/" + file.ID
	r.showCapturedPreview(file, descriptor)
}

// showCapturedPreview loads a captured file into its preview, as it opens or
// as its document shows another version.
func (r *Runtime) showCapturedPreview(file conversation.File, descriptor attachmentDescriptor) {
	file.State = conversation.PreviewLoading
	ctx := r.attachments.ctx
	var preview *filePreview
	var tab *documentPreview
	var document *documentImages
	if file.Kind == conversation.ImageFile {
		preview, ctx = r.beginFilePreview(file, fileview.Callbacks{Image: func(action kit.ImageAction, _ conversation.Image) {
			r.enqueue(func(c *ui.Context) { r.attachmentImageDescriptorAction(c, action, descriptor) })
		}})
		preview.worktree = descriptor.worktree
	} else {
		if r.attachments.previewDocuments == nil {
			r.attachments.previewDocuments = map[string]*documentPreview{}
		}
		tab = r.attachments.previewDocuments[file.ID]
		if tab == nil {
			tab = &documentPreview{}
			r.attachments.previewDocuments[file.ID] = tab
		} else if tab.loading != nil {
			tab.loading.cancel()
		}
		var version string
		if descriptor.revision != nil {
			version = *descriptor.revision
		} else if descriptor.artifact != nil {
			version = fmt.Sprint(descriptor.artifact.Revision)
		} else if descriptor.attachment != nil {
			version = descriptor.attachment.Spec.Revision
		}
		document = newDocumentImages(ctx, r, descriptor, version)
		tab.loading = document
		ctx = document.ctx
		r.App.RevealConversationPanel()
		if tab.shown == nil {
			r.attachments.previews[file.ID] = descriptor
			r.App.Panel.ShowArtifact(file)
		} else {
			r.App.Panel.Show(panel.Tab{Kind: panel.Artifact, ID: file.ID})
		}
	}
	r.async(func() {
		loaded, content, err := r.readAttachmentPreview(ctx, descriptor, file)
		if content != nil && err == nil && document != nil {
			content.Html = document.inlineHTML(content.Html)
		}
		r.enqueue(func(c *ui.Context) {
			if preview != nil {
				root := r.localRuntime()
				if root.filePreview != preview || !preview.open || ctx.Err() != nil {
					if loaded.Image != file.Image {
						releasePreviewImage(loaded.Image)
					}
					return
				}
				if err != nil {
					r.downloadOutcome(c, err)
					root.invalidateFilePreview(nil)
				} else {
					preview.file = loaded
				}
				return
			}
			if tab.loading != document || ctx.Err() != nil {
				return
			}
			tab.loading = nil
			if err != nil || loaded.State == conversation.PreviewLarge {
				if err != nil {
					r.downloadOutcome(c, err)
				} else {
					kit.Toast(c, kit.Failed, i18n.T(c, "artifact_preview_large"))
				}
				document.cancel()
				if tab.shown == nil {
					r.forgetDocumentPreview(file.ID)
				}
				return
			}
			if tab.shown != nil {
				tab.shown.cancel()
			}
			tab.shown = document
			if content != nil {
				r.attachments.previewContent[file.ID] = *content
				if descriptor.artifact != nil {
					// The version read is the one the preview's actions use.
					descriptor.artifact = &content.Artifact
				}
			} else {
				delete(r.attachments.previewContent, file.ID)
				version := fmt.Sprintf("%x", blake3.Sum256([]byte(loaded.Text)))
				document.version, document.origin.version = version, version
			}
			r.attachments.previews[file.ID] = descriptor
			r.App.Panel.UpdateArtifact(loaded)
		})
	})
}

func (r *Runtime) artifactMarkdown(file conversation.File) *markdown.Options {
	owner := r.artifactRuntime(file.ID)
	if owner == nil || owner.attachments == nil {
		return nil
	}
	if tab := owner.attachments.previewDocuments[file.ID]; tab != nil && tab.shown != nil {
		return tab.shown.options()
	}
	return nil
}

func (r *Runtime) pruneDocumentPreviews() {
	if r.parent == nil && r.browser != nil && len(r.browser.sources) > 0 && r.App != nil {
		keep := map[string]bool{}
		for _, tab := range r.App.AllPanelTabs() {
			if (tab.Kind == panel.Documents || tab.Kind == panel.Artifact) && tab.ID != "" {
				keep[tab.ID] = true
			}
		}
		r.pruneBrowserSources(keep)
	}
	if r.attachments == nil || len(r.attachments.previewDocuments) == 0 {
		return
	}
	open := map[string]bool{}
	root := r.localRuntime()
	for _, tab := range root.App.AllPanelTabs() {
		if tab.Kind == panel.Artifact {
			open[tab.ID] = true
		}
	}
	for id := range r.attachments.previewDocuments {
		if !open[id] {
			r.forgetDocumentPreview(id)
		}
	}
}

func (r *Runtime) forgetDocumentPreview(id string) {
	if tab := r.attachments.previewDocuments[id]; tab != nil {
		if tab.loading != nil {
			tab.loading.cancel()
		}
		if tab.shown != nil {
			tab.shown.cancel()
		}
	}
	delete(r.attachments.previewDocuments, id)
	delete(r.attachments.previewContent, id)
	delete(r.attachments.previews, id)
	root := r.localRuntime()
	if root.browser != nil {
		delete(root.browser.sources, id)
	}
	app := root.App
	app.DropPanelTab(panel.Tab{Kind: panel.Artifact, ID: id})
	app.Panel.ForgetArtifact(id)
}

func (r *Runtime) invalidateDocumentPreviews(worktree *protocol.WorktreeID) {
	if r.attachments == nil {
		return
	}
	for id := range r.attachments.previewDocuments {
		if worktree == nil || r.attachments.previews[id].worktree == *worktree {
			r.forgetDocumentPreview(id)
		}
	}
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
		data, _, err := r.readAttachmentImage(ctx, descriptor)
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
	if htmlFile(descriptor.path) {
		// A delivered page renders as a published document does; its
		// source stays the text that Copy and the code fallback use.
		return file, &protocol.ArtifactContent{Html: file.Text}, nil
	}
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
	// A revision never changes, so its ID and number name it; a version
	// chosen in the preview is known only by those.
	if !ok || output.Kind != "artifact" || content.Artifact.ID != artifact.ID || content.Artifact.Revision != artifact.Revision {
		return protocol.ArtifactContent{}, fileActionFault(protocol.ErrorCodeRevisionConflict, "published document revision changed")
	}
	return content, nil
}

// OpenArtifactExternal and ArtifactImageAction receive only captured preview IDs.
func (r *Runtime) OpenArtifactExternal(file conversation.File) {
	r.enqueue(func(c *ui.Context) {
		owner := r.artifactRuntime(file.ID)
		if owner == nil {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "file preview source is unavailable"))
			return
		}
		descriptor, ok := owner.attachments.previews[file.ID]
		if !ok {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "file preview source is unavailable"))
			return
		}
		if descriptor.artifact != nil {
			owner.openExternalArtifact(descriptor, documentFileMount(c))
			return
		}
		owner.openExternalAttachment(descriptor, false)
	})
}
func (r *Runtime) ArtifactImageAction(action kit.ImageAction, image conversation.Image) {
	r.enqueue(func(c *ui.Context) {
		owner := r.artifactRuntime(image.ID)
		if owner == nil {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "image preview source is unavailable"))
			return
		}
		descriptor, ok := owner.attachments.previews[image.ID]
		if !ok {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "image preview source is unavailable"))
			return
		}
		owner.attachmentImageDescriptorAction(c, action, descriptor)
	})
}
func (r *Runtime) chooseArtifactDestination(descriptor attachmentDescriptor, mount documentMount) {
	native := r.attachments.native
	if native.SaveFile == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native save panel is unavailable"))
		return
	}
	native.SaveFile(filepath.Base(descriptor.name), func(path string) {
		if path == "" {
			return
		}
		r.writePublishedArtifact(descriptor, mount, path, nil)
	})
}
func (r *Runtime) writePublishedArtifact(descriptor attachmentDescriptor, mount documentMount, path string, finished func(error)) {
	r.async(func() {
		content, err := r.readPublishedArtifact(r.attachments.ctx, descriptor)
		if err == nil {
			target, e := prepareDownloadDestination(path)
			err = e
			if e == nil {
				defer target.close()
				_, err = target.file.WriteString(documentFile(content, mount))
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
func (r *Runtime) openExternalArtifact(descriptor attachmentDescriptor, mount documentMount) {
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
		r.writePublishedArtifact(descriptor, mount, path, func(err error) {
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
	state := r.attachments
	native := state.native
	if artifact == nil || artifact.Kind != protocol.ArtifactKindPreview || native.RenderDocument == nil {
		return nil
	}
	key := fmt.Sprintf("%s/%s/%d", session, artifact.ID, artifact.Revision)
	image := state.documents[key]
	if image == nil {
		if len(state.documents) >= 48 {
			return nil
		}
		image = &attachmentDocumentImage{}
		state.documents[key] = image
	}
	image.visible = true
	if image.image != nil || image.loading || image.failed || !r.controller.Connected() {
		return image.image
	}
	ctx, cancel := context.WithCancel(r.attachmentImages(session).ctx)
	if r.loadAttachmentThumbnail(ctx, documentThumbnailEdge, func(finish attachmentThumbnailResult) {
		content, err := r.readPublishedArtifact(ctx, descriptor)
		if err != nil {
			finish(nil, err)
			return
		}
		r.enqueue(func(*ui.Context) {
			if err := ctx.Err(); err != nil {
				finish(nil, err)
				return
			}
			native.RenderDocument(ctx, *artifact, content.Html, func(data []byte, err error) {
				if err != nil {
					finish(nil, err)
					return
				}
				if err := ctx.Err(); err != nil {
					finish(nil, err)
					return
				}
				if len(data) > 8<<20 {
					finish(nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "document thumbnail exceeds its byte limit"))
					return
				}
				if !r.async(func() {
					if err := ctx.Err(); err != nil {
						finish(nil, err)
						return
					}
					bitmap, err := decodeAttachmentBitmap(data, documentThumbnailEdge)
					finish(&attachmentThumbnail{bitmap: bitmap}, err)
				}) {
					finish(nil, context.Canceled)
				}
			})
		})
	}, func(thumbnail *attachmentThumbnail, err error) bool {
		if ctx.Err() != nil || state.documents[key] != image {
			return false
		}
		cancel()
		image.loading, image.failed, image.cancel = false, err != nil, nil
		if err != nil {
			return false
		}
		width, height := thumbnail.bitmap.Size()
		image.thumbnail = thumbnail
		image.image = &conversation.Image{ID: id, Name: descriptor.name, Width: width, Height: height, Source: thumbnail.bitmap}
		return true
	}) {
		image.loading, image.cancel = true, cancel
	} else {
		cancel()
	}
	return nil
}

func (r *Runtime) renderArtifactPreview(c *ui.Context, file conversation.File) bool {
	if owner := r.artifactRuntime(file.ID); owner != nil && owner != r {
		return owner.renderArtifactPreview(c, file)
	}
	if r.attachments == nil {
		return false
	}
	if r.attachments.native.DocumentView == nil {
		return false
	}
	descriptor, ok := r.attachments.previews[file.ID]
	content, loaded := r.attachments.previewContent[file.ID]
	if !ok || !loaded {
		return false
	}
	if descriptor.artifact == nil {
		if !htmlFile(descriptor.path) {
			return false
		}
		return r.documentView(c, documentSource{content: content, page: file.ID})
	}
	return r.attachments.native.DocumentView(c, documentSource{content: content, owner: r, conversation: descriptor.session})
}

// htmlFile reports a worktree page the preview renders.
func htmlFile(name string) bool {
	extension := strings.ToLower(filepath.Ext(name))
	return extension == ".html" || extension == ".htm"
}

func (r *Runtime) artifactRuntime(id string) *Runtime {
	root := r.localRuntime()
	if root.attachments != nil {
		if _, ok := root.attachments.previews[id]; ok {
			return root
		}
	}
	for _, bound := range root.bindings {
		if bound.attachments != nil {
			if _, ok := bound.attachments.previews[id]; ok {
				return bound
			}
		}
	}
	return nil
}

// artifactVersions are the version a document's preview shows and the
// latest its conversation sees; none for a file or before it loads.
func (r *Runtime) artifactVersions(file conversation.File) (uint32, uint32) {
	if owner := r.artifactRuntime(file.ID); owner != nil && owner != r {
		return owner.artifactVersions(file)
	}
	if r.attachments == nil || r.attachments.previews[file.ID].artifact == nil {
		return 0, 0
	}
	content, loaded := r.attachments.previewContent[file.ID]
	if !loaded {
		return 0, 0
	}
	return content.Artifact.Revision, content.LatestRevision
}

// showArtifactVersion loads another version of a document into its preview.
// Only the latest takes form answers.
func (r *Runtime) showArtifactVersion(file conversation.File, revision uint32) {
	r.enqueue(func(*ui.Context) {
		owner := r.artifactRuntime(file.ID)
		if owner == nil || owner.attachments == nil {
			return
		}
		descriptor, ok := owner.attachments.previews[file.ID]
		if !ok || descriptor.artifact == nil {
			return
		}
		artifact := *descriptor.artifact
		artifact.Revision = revision
		descriptor.artifact = &artifact
		owner.showCapturedPreview(file, descriptor)
	})
}
