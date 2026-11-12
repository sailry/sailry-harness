package live

import (
	"context"
	"fmt"
	"math"
	"path"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type attachmentPublication struct {
	session        protocol.SessionID
	loading, ready bool
	failed         bool
	items          []attachmentPublishedItem
}
type attachmentPublishedItem struct {
	file       conversation.File
	descriptor attachmentDescriptor
	image      bool
}

func attachmentFileKind(name, media string) conversation.FileKind {
	switch media {
	case "image/png", "image/jpeg", "image/gif", "image/webp", "image/bmp", "image/tiff":
		return conversation.ImageFile
	}
	switch strings.ToLower(path.Ext(name)) {
	case ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tif", ".tiff":
		return conversation.ImageFile
	case ".md", ".markdown", ".mdown":
		return conversation.MarkdownFile
	}
	if editor.LanguageFor(name) != nil {
		return conversation.CodeFile
	}
	if strings.HasPrefix(media, "text/") || media == "application/json" || media == "application/xml" {
		return conversation.TextFile
	}
	return conversation.OtherFile
}
func sentAttachmentImages(turn *conversation.Turn) {
	files := turn.Files[:0]
	for _, file := range turn.Files {
		file.Kind = attachmentFileKind(file.Path, file.Mime)
		if file.Kind == conversation.ImageFile {
			if !slices.ContainsFunc(turn.Images, func(image conversation.Image) bool { return image.ID == file.ID }) {
				turn.Images = append(turn.Images, conversation.Image{ID: file.ID, Name: file.Name()})
			}
		} else {
			files = append(files, file)
		}
	}
	turn.Files = files
}

// projectConversationAssets projects only publication metadata. Raw tool details
// remain in Client's bounded shared cache; file bytes load only for previews.
func (r *Runtime) projectConversationAssets(cv *viewmodel.Conversation, state client.ConversationState) {
	if r.attachments == nil {
		return
	}
	for index := range cv.Turns {
		sentAttachmentImages(&cv.Turns[index])
		cv.Turns[index].Reply.Blocks = slices.DeleteFunc(cv.Turns[index].Reply.Blocks, func(block conversation.Block) bool { return strings.HasPrefix(block.Label, "publication/") })
	}
	page := state.Snapshot.Page
	type owner struct {
		turn     string
		worktree protocol.WorktreeID
	}
	owners := map[protocol.TurnID]owner{}
	displayed := turnOwners(page.Runs)
	for _, run := range page.Runs {
		owners[run.Turn] = owner{turn: displayed[run.Turn].String(), worktree: run.Worktree}
	}
	positions := map[string]int{}
	for index, turn := range cv.Turns {
		positions[turn.ID] = index
	}
	prefetch := state.Connected && r.attachmentConversationVisible(cv.ID)
	seen := map[string]bool{}
	blocks := map[string][]conversation.Block{}
	retained := map[string]bool{}
	for ei := len(page.Entries) - 1; ei >= 0; ei-- {
		entry := page.Entries[ei]
		location, ok := owners[entry.Turn]
		if !ok {
			continue
		}
		if _, ok = positions[location.turn]; !ok {
			continue
		}
		for pi := len(entry.Parts) - 1; pi >= 0; pi-- {
			part := entry.Parts[pi]
			result, ok := detail[protocol.ConversationPartToolResultPayload](part.Data)
			if part.Kind != "tool_result" || !ok || result.Summary == nil || !result.Summary.Published || !result.Summary.HasDetails {
				continue
			}
			ref := protocol.ToolDetailReference{Session: page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(pi), Call: result.ID, Revision: result.Summary.Revision}
			key := fmt.Sprintf("publication/%s/%s/%d/%s", page.Session, entry.ID, pi, result.Summary.Revision)
			retained[key] = true
			publication := r.attachments.publications[key]
			if publication == nil {
				publication = &attachmentPublication{session: page.Session}
				r.attachments.publications[key] = publication
			}
			if !publication.ready {
				if loaded, ok := r.controller.ConversationDetail(ref); ok {
					publication.items = projectPublishedItems(ref, location.worktree, loaded)
					publication.ready, publication.loading = true, false
				} else if prefetch && !publication.loading && !publication.failed {
					publication.loading = true
					r.loadAttachmentPublication(r.attachmentImages(cv.ID).ctx, key, ref, location.worktree, publication)
				}
			}
			for index := len(publication.items) - 1; index >= 0; index-- {
				item := publication.items[index]
				dedupe := location.turn + "/file/" + item.descriptor.path
				if item.descriptor.artifact != nil {
					dedupe = location.turn + "/document/" + item.descriptor.artifact.ID.String()
				}
				if seen[dedupe] {
					continue
				}
				seen[dedupe] = true
				r.attachments.files[attachmentKey(cv.ID, item.file.ID)] = item.descriptor
				if item.descriptor.artifact != nil && prefetch {
					item.file.Image = r.projectDocumentThumbnail(cv.ID, item.file.ID, item.descriptor)
				}
				block := conversation.Block{Kind: conversation.FilesBlock, Label: key, Files: []conversation.File{item.file}}
				if item.image {
					block.Kind, block.Files = conversation.ImagesBlock, nil
					block.Images = []conversation.Image{{ID: item.file.ID, Name: item.file.Name()}}
				}
				blocks[location.turn] = append(blocks[location.turn], block)
			}
		}
	}
	for turn, published := range blocks {
		slices.Reverse(published)
		index := positions[turn]
		cv.Turns[index].Reply.Blocks = append(cv.Turns[index].Reply.Blocks, published...)
	}
	for key, publication := range r.attachments.publications {
		if publication.session == page.Session && !retained[key] {
			delete(r.attachments.publications, key)
		}
	}
}
func (r *Runtime) loadAttachmentPublication(ctx context.Context, key string, ref protocol.ToolDetailReference, worktree protocol.WorktreeID, publication *attachmentPublication) {
	r.async(func() {
		loaded, err := r.controller.ReadConversationDetail(ctx, ref)
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() != nil || r.attachments.publications[key] != publication {
				return
			}
			publication.loading, publication.failed = false, err != nil
			if err != nil {
				r.downloadOutcome(c, err)
				return
			}
			publication.items = projectPublishedItems(ref, worktree, loaded)
			publication.ready = true
			r.projectConversations()
		})
	})
}
func projectPublishedItems(ref protocol.ToolDetailReference, worktree protocol.WorktreeID, loaded protocol.ToolDetail) []attachmentPublishedItem {
	items := []attachmentPublishedItem{}
	if loaded.Content == nil {
		return items
	}
	for index, block := range loaded.Content.Blocks {
		id := fmt.Sprintf("published/%s/%s/%d/%d", ref.Turn, ref.Entry, ref.Index, index)
		switch block.Kind {
		case "file":
			file, ok := detail[protocol.ToolContentFile](block.Data)
			if !ok {
				continue
			}
			kind := attachmentFileKind(file.Path, file.Mime)
			item := attachmentPublishedItem{file: conversation.File{ID: id, Path: file.Path, Mime: file.Mime, Kind: kind, Editable: kind == conversation.TextFile || kind == conversation.CodeFile || kind == conversation.MarkdownFile}, descriptor: attachmentDescriptor{session: ref.Session, worktree: worktree, path: file.Path, name: path.Base(file.Path), mime: file.Mime, revision: cloneAttachmentRevision(file.Revision)}, image: kind == conversation.ImageFile}
			if file.Size != nil && *file.Size <= math.MaxInt64 {
				item.file.Size = int64(*file.Size)
			}
			items = append(items, item)
		case "artifact":
			value, ok := detail[protocol.ToolContentBlockArtifactPayload](block.Data)
			if !ok {
				continue
			}
			artifact := value.Artifact
			name := artifact.Title
			if path.Ext(name) == "" {
				name += ".html"
			}
			items = append(items, attachmentPublishedItem{file: conversation.File{ID: id, Path: name, Mime: "text/html", Kind: conversation.MarkdownFile, Size: int64(min(artifact.Size, math.MaxInt64)), Document: true, Interactive: artifact.Kind != protocol.ArtifactKindDocument}, descriptor: attachmentDescriptor{session: ref.Session, worktree: worktree, name: name, mime: "text/html", artifact: &artifact}})
		}
	}
	return items
}
