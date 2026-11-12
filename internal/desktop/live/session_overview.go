package live

import (
	"fmt"
	"math"
	"path"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// overviewLimit is how many asset groups one read of the overview asks for.
const overviewLimit = 20

// overviewRead is a session's asset inventory as the overview popover read
// it: the groups of one structural history revision, newest first, kept
// while the session is not shown so it reopens at once.
type overviewRead struct {
	open bool
	// revision and head are the history the popover was last synced with.
	revision, head uint64
	// loaded is false until a read succeeded; groups were read at
	// loadedRevision when the history's newest sequence was loadedHead.
	loaded                     bool
	loadedRevision, loadedHead uint64
	groups                     []protocol.ConversationAssetsGroup
	before                     *uint64
	loading, failed            bool
	// generation names the read under way: a newer read or a close drops
	// an older one's result.
	generation uint64
}

// current reports whether the inventory covers the newest history.
func (o *overviewRead) current() bool {
	return o.loaded && o.loadedRevision == o.revision && o.loadedHead == o.head
}

// sessionOverview is the header's overview of conversation cv: the running
// subagents, the current turn's progress and cited pages from the
// transcript, and the Host's inventory of outputs and sources.
func (r *Runtime) sessionOverview(c *ui.Context, cv *viewmodel.Conversation) *composer.Overview {
	owner := r.conversationRuntime(cv, r.App.Composer(cv.ID))
	if owner == nil {
		return &composer.Overview{}
	}
	if owner != r {
		return owner.sessionOverview(c, cv)
	}
	session, ok := r.session(cv.ID)
	if !ok {
		return &composer.Overview{}
	}
	if r.overviews == nil {
		r.overviews = map[protocol.SessionID]*overviewRead{}
	}
	read := r.overviews[session.ID]
	if read == nil {
		read = &overviewRead{}
		r.overviews[session.ID] = read
	}
	view := &composer.Overview{Session: true}
	connected := r.controller.Connected()
	if state, ready := r.conversationState(session.ID); ready {
		page := &state.Snapshot.Page
		for _, child := range client.LatestConversationChildren(page.Children) {
			if child.Run.Status != protocol.ConversationStatusRunning {
				continue
			}
			if childSession, valid := client.ConversationChildSession(r.snapshot(), session.ID, child); valid {
				view.Children = append(view.Children, composer.OverviewChild{ID: childSession.ID.String(),
					Name: childTitle(page, child, c.Locale()), Status: "chat_running", Running: true, Avatar: childAvatar(r.snapshot().Roles, child)})
			}
		}
		if read.revision != page.Revision || read.head != state.Snapshot.Sequence {
			// The history moved: a read under way is for the old one, and
			// a structural change makes the rows read so far another
			// history's.
			read.generation++
			read.loading, read.failed = false, false
			read.revision, read.head = page.Revision, state.Snapshot.Sequence
			if read.loaded && read.loadedRevision != read.revision {
				read.loaded, read.groups, read.before = false, nil, nil
			}
			if read.open && !read.current() {
				r.loadOverview(session.ID, read, false)
			}
		}
	}
	// The current turn is the one running, or the last that started.
	current := -1
	for i := len(cv.Turns) - 1; i >= 0; i-- {
		status := cv.Turns[i].Reply.Status
		if cv.Turns[i].Compaction != nil || status == conversation.Queued {
			continue
		}
		if status.Active() {
			current = i
			break
		}
		if current < 0 {
			current = i
		}
	}
	if current >= 0 {
		turn := cv.Turns[current]
		view.Active = connected && turn.Reply.Status.Active()
		seen := map[string]bool{}
		cite := func(list []conversation.Citation) {
			for _, source := range list {
				if !seen[source.URI] {
					seen[source.URI] = true
					view.Citations = append(view.Citations, source)
				}
			}
		}
		for _, block := range turn.Reply.Blocks {
			if block.Kind == conversation.ProgressBlock {
				view.Steps = block.Steps
			}
			cite(block.Sources)
		}
		cite(turn.Reply.Citations)
	}
	rows := overviewAssets(read.groups)
	for _, row := range rows {
		if row.group.Kind == protocol.ConversationAssetsKindArtifact {
			view.Output = append(view.Output, row.view)
		} else {
			view.Sources = append(view.Sources, row.view)
		}
	}
	view.Loading, view.Failed, view.More = read.loading, read.failed, read.loaded && read.before != nil
	view.Opened = func(open bool) {
		read.open = open
		if open {
			if !read.current() {
				r.loadOverview(session.ID, read, false)
			}
			return
		}
		read.generation++
		read.loading = false
	}
	view.Load = func(more bool) { r.loadOverview(session.ID, read, more) }
	view.OpenChild = func(id string) { r.openLiveChild(cv, id) }
	view.OpenSource = func(uri string) {
		if !r.localRuntime().openChatLink(c, cv.ID, uri) && ui.SafeToOpen(uri) {
			c.OpenURL(uri)
		}
	}
	view.OpenAsset = func(id string) {
		for _, row := range rows {
			if row.view.ID == id {
				r.openOverviewAsset(session, row)
			}
		}
	}
	return view
}

// loadOverview reads the session's inventory: its first page, or the next
// one after what is loaded. A page read at another history revision than
// the one shown fails, as the rows would not belong together.
func (r *Runtime) loadOverview(session protocol.SessionID, read *overviewRead, more bool) {
	var before *uint64
	if more {
		before = read.before
	}
	read.generation++
	generation, head := read.generation, read.head
	read.loading, read.failed = true, false
	r.async(func() {
		page, err := r.readConversationAssets(session, before)
		r.enqueue(func(c *ui.Context) {
			if read.generation != generation {
				return
			}
			read.loading = false
			if err != nil || read.revision != 0 && read.revision != page.Revision || more && read.loaded && read.loadedRevision != page.Revision {
				read.failed = true
				kit.Toast(c, kit.Failed, i18n.T(c, "chat_assets_failed"))
				return
			}
			if !more {
				read.groups = nil
			}
			read.groups = append(read.groups, page.Groups...)
			read.loaded, read.loadedRevision, read.loadedHead, read.before = true, page.Revision, head, page.NextBefore
		})
	})
}

// readConversationAssets asks the Host for groups until it has overviewLimit
// of them or the inventory ends: one answer may hold fewer.
func (r *Runtime) readConversationAssets(session protocol.SessionID, before *uint64) (protocol.ConversationAssetsPage, error) {
	result := protocol.ConversationAssetsPage{Session: session}
	for {
		query := protocol.ConversationAssetsQuery{Before: before, Limit: uint16(overviewLimit - len(result.Groups))}
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_conversation_assets", Data: protocol.CommandListConversationAssetsPayload{Session: session, Query: query}})
		if err != nil {
			return result, err
		}
		page, valid := detail[protocol.ConversationAssetsPage](output.Data)
		if !valid || output.Kind != "conversation_assets" || page.Session != session || page.Revision == 0 || len(page.Groups) > int(query.Limit) {
			return result, fileActionFault(protocol.ErrorCodeInvalidRequest, "conversation asset response is invalid")
		}
		if result.Revision != 0 && result.Revision != page.Revision {
			return result, fileActionFault(protocol.ErrorCodeRevisionConflict, "conversation history changed while reading assets")
		}
		result.Revision, result.NextBefore = page.Revision, page.NextBefore
		result.Groups = append(result.Groups, page.Groups...)
		if len(result.Groups) >= overviewLimit || result.NextBefore == nil {
			return result, nil
		}
		before = result.NextBefore
	}
}

// overviewAsset is a row of the inventory with what it opens.
type overviewAsset struct {
	view   composer.OverviewAsset
	group  protocol.ConversationAssetsGroup
	target protocol.ConversationAssetsTarget
}

// overviewAssets are the inventory's rows, the newest occurrence of each
// target: outputs and sources apart, and files of different worktrees apart.
// groups are newest first.
func overviewAssets(groups []protocol.ConversationAssetsGroup) []overviewAsset {
	var rows []overviewAsset
	seen := map[string]bool{}
	for _, group := range groups {
		for index, target := range group.Items {
			name, mime, identity := "", "", ""
			worktree := group.Worktree.String()
			switch target.Kind {
			case "attachment":
				value, ok := detail[protocol.Attachment](target.Data)
				if !ok {
					continue
				}
				name, mime, identity = value.Spec.Name, value.Spec.MediaType, "attachment/"+value.ID.String()
			case "image":
				value, ok := detail[protocol.ConversationImage](target.Data)
				if !ok {
					continue
				}
				name, mime = value.Attachment.Spec.Name, value.Attachment.Spec.MediaType
				identity = fmt.Sprintf("image/%s/%d/%d", value.Entry, value.Part, value.Index)
			case "document":
				value, ok := detail[protocol.ArtifactSummary](target.Data)
				if !ok {
					continue
				}
				name, mime, identity, worktree = value.Title, "text/html", "document/"+value.ID.String(), ""
			case "file":
				value, ok := detail[protocol.ConversationAssetsTargetFilePayload](target.Data)
				if !ok {
					continue
				}
				name, identity = value.Path, "file/"+value.Path
			default:
				continue
			}
			key := string(group.Kind) + "\x00" + worktree + "\x00" + identity
			if seen[key] {
				continue
			}
			seen[key] = true
			row := overviewAsset{group: group, target: target}
			row.view = composer.OverviewAsset{ID: fmt.Sprintf("%d-%d", group.Sequence, index), Name: name,
				Icon: conversation.FileIcon(conversation.File{Kind: attachmentFileKind(name, mime)})}
			if group.Kind == protocol.ConversationAssetsKindResource {
				// A source shows its last element, its path as the tooltip.
				row.view.Label = path.Base(strings.ReplaceAll(name, "\\", "/"))
			}
			rows = append(rows, row)
		}
	}
	return rows
}

// openOverviewAsset opens a row: an attachment, picture, document or output
// file in the panel's preview, a source file in the workspace's Files tab.
func (r *Runtime) openOverviewAsset(session protocol.Session, row overviewAsset) {
	id := "overview/" + row.view.ID
	file := conversation.File{ID: id, Path: row.view.Name}
	descriptor := attachmentDescriptor{session: session.ID, worktree: row.group.Worktree, name: row.view.Name}
	switch row.target.Kind {
	case "attachment":
		value, _ := detail[protocol.Attachment](row.target.Data)
		descriptor.worktree, descriptor.mime, descriptor.attachment = value.Spec.Worktree, value.Spec.MediaType, &value
		file.Size = int64(min(value.Spec.Size, math.MaxInt64))
	case "image":
		value, _ := detail[protocol.ConversationImage](row.target.Data)
		attachment := value.Attachment
		descriptor.worktree, descriptor.mime, descriptor.attachment, descriptor.image = attachment.Spec.Worktree, attachment.Spec.MediaType, &attachment, &value
		file.Size = int64(min(attachment.Spec.Size, math.MaxInt64))
	case "document":
		value, _ := detail[protocol.ArtifactSummary](row.target.Data)
		if path.Ext(file.Path) == "" {
			file.Path += ".html"
		}
		descriptor.name, descriptor.mime, descriptor.artifact = file.Path, "text/html", &value
		file.Mime, file.Kind, file.Size = "text/html", conversation.MarkdownFile, int64(min(value.Size, math.MaxInt64))
	case "file":
		value, _ := detail[protocol.ConversationAssetsTargetFilePayload](row.target.Data)
		if row.group.Kind == protocol.ConversationAssetsKindResource && attachmentFileKind(value.Path, "") != conversation.ImageFile {
			r.openWorkspaceFile(session.ID, row.group.Worktree, value.Path)
			return
		}
		descriptor.path = value.Path
	}
	if r.attachments == nil {
		return
	}
	file.Mime = descriptor.mime
	r.attachments.files[attachmentKey(session.ID.String(), id)] = descriptor
	r.openCapturedFilePreview(session.ID.String(), file, descriptor)
}
