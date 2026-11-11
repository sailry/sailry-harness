package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ListConversationAssets derives bounded inventory pages from canonical history.
// It does not index workspace writes or inspect the filesystem.
func ListConversationAssets(ctx context.Context, db *sql.Conn, session protocol.SessionID, query protocol.ConversationAssetsQuery) (protocol.ConversationAssetsPage, error) {
	page := protocol.DefaultConversationAssetsPage()
	page.Session = session
	if err := requireSession(ctx, db, session); err != nil {
		return page, err
	}
	if query.Limit < 1 || query.Limit > 50 || query.Before != nil && (*query.Before == 0 || *query.Before > math.MaxInt64) {
		return page, invalid("conversation asset bounds are invalid")
	}
	if query.Before != nil {
		var visible bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM conversation_events WHERE session=?1 AND sequence=?2)", session.String(), *query.Before).Scan(&visible); err != nil {
			return page, err
		}
		if !visible {
			return page, fault(protocol.ErrorCodeWrongTarget, "asset cursor is not visible in this session")
		}
	}
	var err error
	if page.Revision, err = historyRevision(ctx, db, session); err != nil {
		return page, err
	}
	boundary := uint64(math.MaxInt64)
	if query.Before != nil {
		boundary = *query.Before
	}
	rows, err := db.QueryContext(ctx, "SELECT e.sequence,e.turn,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND h.sequence<?2 ORDER BY h.sequence DESC LIMIT 257", session.String(), boundary)
	if err != nil {
		return page, err
	}
	type event struct {
		sequence uint64
		turn     string
		body     []byte
	}
	records := []event{}
	bytes := 0
	more := false
	for rows.Next() {
		if len(records) == 256 || bytes >= 4*1024*1024 {
			more = true
			break
		}
		var value event
		if err = rows.Scan(&value.sequence, &value.turn, &value.body); err != nil {
			rows.Close()
			return page, err
		}
		records = append(records, value)
		bytes += len(value.body)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return page, err
	}
	type location struct {
		worktree protocol.WorktreeID
		root     *string
	}
	locations := map[protocol.TurnID]location{}
	var last uint64
	for _, value := range records {
		if len(page.Groups) == int(query.Limit) {
			page.NextBefore = &last
			return page, nil
		}
		last = value.sequence
		turn, err := protocol.ParseTurnID(value.turn)
		if err != nil {
			return page, err
		}
		var record canonical.Record
		if err = json.Unmarshal(value.body, &record); err != nil {
			return page, err
		}
		kind := protocol.ConversationAssetsKindArtifact
		if record.Provenance.Author == "user" {
			kind = protocol.ConversationAssetsKindResource
		}
		if query.Kind != nil && *query.Kind != kind {
			continue
		}
		owner, ok := locations[turn]
		if !ok {
			run, err := visibleAgentRun(ctx, db, session, turn)
			if err != nil {
				return page, err
			}
			owner.worktree = run.Worktree
			var path string
			err = db.QueryRowContext(ctx, "SELECT path FROM worktrees WHERE id=?1", run.Worktree.String()).Scan(&path)
			if err == nil {
				owner.root = &path
			} else if !errors.Is(err, sql.ErrNoRows) {
				return page, err
			}
			locations[turn] = owner
		}
		items, err := conversationAssetItems(ctx, db, record, owner.worktree, owner.root)
		if err != nil {
			return page, err
		}
		if len(items) > 0 {
			page.Groups = append(page.Groups, protocol.ConversationAssetsGroup{Sequence: value.sequence, Turn: turn, Worktree: owner.worktree, TimestampMs: record.Created.UnixMilli(), Kind: kind, Items: items})
		}
	}
	if more {
		page.NextBefore = &last
	}
	return page, nil
}

type conversationAssetTargets struct {
	items []protocol.ConversationAssetsTarget
	seen  map[string]bool
	err   error
}

func (targets *conversationAssetTargets) push(kind string, data any) {
	if targets.err != nil {
		return
	}
	value := protocol.ConversationAssetsTarget{Kind: kind, Data: data}
	body, err := encode(value)
	if err != nil {
		targets.err = err
		return
	}
	if targets.seen == nil {
		targets.seen = map[string]bool{}
	}
	if !targets.seen[string(body)] {
		targets.items = append(targets.items, value)
		targets.seen[string(body)] = true
	}
}
func (targets *conversationAssetTargets) file(path string) {
	targets.push("file", &protocol.ConversationAssetsTargetFilePayload{Path: path})
}
func (targets *conversationAssetTargets) image(record canonical.Record, worktree protocol.WorktreeID, part, index uint64, asset canonical.AssetRef) error {
	if !supportedAgentImage(asset) {
		return nil
	}
	value, err := describeAgentImage(record.Turn, worktree, record.ID, part, index, asset)
	if err == nil {
		targets.push("image", &value)
	}
	return err
}
func conversationAssetItems(ctx context.Context, db *sql.Conn, record canonical.Record, worktree protocol.WorktreeID, root *string) ([]protocol.ConversationAssetsTarget, error) {
	targets := &conversationAssetTargets{items: []protocol.ConversationAssetsTarget{}}
	message := record.Message()
	if message == nil {
		return targets.items, nil
	}
	hidden := record.Provenance.Author == "system" && record.Metadata.Context != nil && record.Metadata.Context.Captured
	if !hidden {
		skipped := uint64(0)
		if input := record.Metadata.Input; input != nil {
			if len(input.References) > 0 {
				skipped++
			}
		}
		for part := skipped; part < message.PartCount; part++ {
			if input := record.Metadata.Input; input != nil && ((input.CapabilityPart != nil && part == *input.CapabilityPart) || (input.NoticePart != nil && part == *input.NoticePart)) {
				continue
			}
			for _, block := range message.Content {
				if block.Index != part {
					continue
				}
				switch value := block.Data.(type) {
				case *canonical.TextContent:
					conversationAssetMarkdown(targets, value.Text, root, record.Provenance.Author == "user")
				case *canonical.AssetContent:
					if block.Kind == "image" {
						if err := targets.image(record, worktree, part, 0, value.Asset); err != nil {
							return nil, err
						}
					}
				case *canonical.FileContent:
					if strings.HasPrefix(value.File.URI, "sailry-attachment://") {
						id, err := protocol.ParseAttachmentID(strings.TrimPrefix(value.File.URI, "sailry-attachment://"))
						if err != nil {
							return nil, err
						}
						attachment, err := GetTurnAttachment(ctx, db, record.Turn, id)
						if err != nil {
							return nil, err
						}
						if value.File.Mime == nil || *value.File.Mime != attachment.Spec.MediaType {
							return nil, invalid("attachment media type mismatch")
						}
						targets.push("attachment", &attachment)
					}
				case *canonical.ToolResponse:
					for index, asset := range agentImageAssets(value) {
						if err := targets.image(record, worktree, part, uint64(index), asset); err != nil {
							return nil, err
						}
					}
				}
				break
			}
		}
		if input := record.Metadata.Input; input != nil {
			for _, reference := range input.References {
				if reference.Target.Kind == "file" {
					if path, ok := filePayload[string](reference.Target.Data); ok {
						targets.file(path)
					}
				}
			}
		}
	}
	for _, block := range message.Content {
		response, ok := block.Data.(*canonical.ToolResponse)
		if !ok {
			continue
		}
		hydrated, err := HydrateCanonicalPayload(ctx, db, response.Result)
		if err != nil {
			return nil, err
		}
		raw := rawAgentTool{Presentation: protocol.ToolPresentationContent, Result: &hydrated, Format: response.Format}
		for index, asset := range agentImageAssets(response) {
			if !supportedAgentImage(asset) {
				continue
			}
			image, err := describeAgentImage(record.Turn, worktree, record.ID, block.Index, uint64(index), asset)
			if err != nil {
				return nil, err
			}
			raw.Images = append(raw.Images, image)
		}
		if content := raw.content(); content != nil {
			for _, item := range content.Blocks {
				switch item.Kind {
				case "file":
					if file, ok := filePayload[protocol.ToolContentFile](item.Data); ok {
						targets.file(file.Path)
					}
				case "artifact":
					if value, ok := filePayload[protocol.ToolContentBlockArtifactPayload](item.Data); ok {
						targets.push("document", &value.Artifact)
					}
				}
			}
		}
	}
	return targets.items, targets.err
}
