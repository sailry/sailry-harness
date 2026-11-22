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
	cancel         context.CancelFunc
	loading, ready bool
	checkedCache   bool
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

func (r *Runtime) projectConversationAssets(cv *viewmodel.Conversation, state client.ConversationState, parts []attachmentPart) {
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
	var owners map[protocol.TurnID]owner
	var positions map[string]int
	prefetch := state.Connected && r.attachmentConversationVisible(cv.ID)
	seen := map[string]bool{}
	blocks := map[string][]conversation.Block{}
	retained := map[string]bool{}
	for i := len(parts) - 1; i >= 0; i-- {
		entry := page.Entries[parts[i].entry]
		pi := parts[i].part
		part := entry.Parts[pi]
		if part.Kind != "tool_result" {
			continue
		}
		result, ok := detail[protocol.ConversationPartToolResultPayload](part.Data)
		if !ok || result.Summary == nil || !result.Summary.Published || !result.Summary.HasDetails {
			continue
		}
		if owners == nil {
			displayed := turnOwners(page.Runs)
			owners = make(map[protocol.TurnID]owner, len(page.Runs))
			for _, run := range page.Runs {
				owners[run.Turn] = owner{turn: displayed[run.Turn].String(), worktree: run.Worktree}
			}
			positions = make(map[string]int, len(cv.Turns))
			for i := range cv.Turns {
				positions[cv.Turns[i].ID] = i
			}
		}
		location, ok := owners[entry.Turn]
		if !ok {
			continue
		}
		if _, ok := positions[location.turn]; !ok {
			continue
		}
		ref := protocol.ToolDetailReference{Session: page.Session, Turn: entry.Turn, Entry: entry.ID, Index: uint64(pi), Call: result.ID, Revision: result.Summary.Revision}
		key := fmt.Sprintf("publication/%s/%s/%d/%s", page.Session, entry.ID, pi, result.Summary.Revision)
		var items []attachmentPublishedItem
		if result.Summary.Publications != nil {
			items = projectPublishedItems(ref, location.worktree, result.Summary.Publications)
		} else {
			retained[key] = true
			publication := r.attachments.publications[key]
			if publication == nil {
				publication = &attachmentPublication{session: page.Session}
				r.attachments.publications[key] = publication
			}
			if !publication.ready && !publication.loading && !publication.failed && (prefetch || !publication.checkedCache) {
				publication.loading, publication.checkedCache = true, true
				r.loadAttachmentPublication(key, ref, location.worktree, publication, prefetch)
			}
			items = publication.items
		}
		for index := len(items) - 1; index >= 0; index-- {
			item := items[index]
			dedupe := location.turn + "/file/" + item.descriptor.path
			if item.descriptor.artifact != nil {
				dedupe = location.turn + "/document/" + item.descriptor.artifact.ID.String()
			}
			if seen[dedupe] {
				continue
			}
			seen[dedupe] = true
			source := attachmentKey(cv.ID, item.file.ID)
			r.attachments.files[source] = item.descriptor
			r.trackTranscriptAttachment(page.Session, source)
			if artifact := item.descriptor.artifact; artifact != nil && artifact.Kind == protocol.ArtifactKindPreview {
				item.file.Image = &conversation.Image{ID: item.file.ID, Name: item.file.Name()}
			}
			block := conversation.Block{Kind: conversation.FilesBlock, Label: key, Files: []conversation.File{item.file}}
			if item.image {
				block.Kind, block.Files = conversation.ImagesBlock, nil
				block.Images = []conversation.Image{{ID: item.file.ID, Name: item.file.Name()}}
			}
			blocks[location.turn] = append(blocks[location.turn], block)
		}
	}
	for turn, published := range blocks {
		slices.Reverse(published)
		index := positions[turn]
		cv.Turns[index].Reply.Blocks = append(cv.Turns[index].Reply.Blocks, published...)
	}
	r.pruneAttachmentPublications(page.Session, retained)
}

func (r *Runtime) pruneAttachmentPublications(session protocol.SessionID, retained map[string]bool) {
	if r.attachments == nil {
		return
	}
	for key, publication := range r.attachments.publications {
		if publication.session == session && !retained[key] {
			if publication.cancel != nil {
				publication.cancel()
			}
			delete(r.attachments.publications, key)
		}
	}
}
func (r *Runtime) loadAttachmentPublication(key string, ref protocol.ToolDetailReference, worktree protocol.WorktreeID, publication *attachmentPublication, fetch bool) {
	ctx, cancel := context.WithCancel(r.attachments.ctx)
	publication.cancel = cancel
	if !r.async(func() {
		loaded, found := r.controller.ConversationDetail(ref)
		var err error
		if fetch && !found {
			loaded, err = r.controller.ReadToolDetail(ctx, ref)
			found = err == nil
		}
		if ctx.Err() != nil {
			return
		}
		var items []attachmentPublishedItem
		if found && loaded.Content != nil {
			var publications []protocol.ToolDetailPublication
			for index, block := range loaded.Content.Blocks {
				if block.Kind == "file" || block.Kind == "artifact" {
					publications = append(publications, protocol.ToolDetailPublication{Index: uint64(index), Block: block})
				}
			}
			items = projectPublishedItems(ref, worktree, publications)
		}
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() != nil || r.attachments.publications[key] != publication {
				return
			}
			cancel()
			publication.cancel = nil
			publication.loading, publication.failed = false, err != nil
			if err != nil {
				r.downloadOutcome(c, err)
				return
			}
			publication.items, publication.ready = items, found
			if !found && !r.attachmentConversationVisible(ref.Session.String()) {
				return
			}
			for cv := range r.conversationPresentations {
				if cv.ID == ref.Session.String() {
					r.invalidateConversationPresentation(cv)
				}
			}
		})
	}) {
		cancel()
		publication.cancel, publication.loading = nil, false
	}
}
func projectPublishedItems(ref protocol.ToolDetailReference, worktree protocol.WorktreeID, publications []protocol.ToolDetailPublication) []attachmentPublishedItem {
	var items []attachmentPublishedItem
	for _, publication := range publications {
		block := publication.Block
		id := fmt.Sprintf("published/%s/%s/%d/%d", ref.Turn, ref.Entry, ref.Index, publication.Index)
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
