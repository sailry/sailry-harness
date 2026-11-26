package store

import (
	"bytes"
	"context"
	"crypto/sha256"
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
	return (*ConversationAssetReads)(nil).List(ctx, db, session, query)
}

func (reads *ConversationAssetReads) List(ctx context.Context, db *sql.Conn, session protocol.SessionID, query protocol.ConversationAssetsQuery) (protocol.ConversationAssetsPage, error) {
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
	entry := assetCheckpoint{session: session, revision: page.Revision}
	if query.Kind != nil {
		entry.kind = *query.Kind
	}
	var prior *assetCheckpoint
	var deps *assetDependencies
	var after uint64
	if reads != nil && query.Checkpoint != nil && (query.Kind == nil || entry.kind == protocol.ConversationAssetsKindResource || entry.kind == protocol.ConversationAssetsKindArtifact) {
		prior = reads.lookup(*query.Checkpoint)
		if prior != nil {
			if prior.session != session || prior.revision != page.Revision || prior.kind != entry.kind || query.Before != nil && (*query.Before < prior.next || *query.Before > prior.head) {
				prior = nil
			} else if valid, err := prior.valid(ctx, db); err != nil {
				return page, err
			} else if !valid {
				prior = nil
			}
		}
		if query.Before != nil && *query.Checkpoint != "" && prior == nil {
			return page, fault(protocol.ErrorCodeRevisionConflict, "retained asset pages changed")
		}
		if query.Before == nil || prior != nil {
			entry.deps = newAssetDependencies()
			deps = &entry.deps
		}
		if prior != nil && query.Before == nil {
			after = prior.head
		}
	}
	boundary := uint64(math.MaxInt64)
	if query.Before != nil {
		boundary = *query.Before - 1
	}
	oldest, more, err := conversationAssetBoundary(ctx, db, session, after, boundary)
	if err != nil {
		return page, err
	}
	var last uint64
	if oldest != 0 {
		rows, err := db.QueryContext(ctx, `SELECT h.sequence,e.turn,e.id,e.body`+conversationAssetMembers, session.String(), boundary, oldest-1)
		if err != nil {
			return page, err
		}
		defer rows.Close()
		locations := map[protocol.TurnID]assetLocation{}
		for rows.Next() {
			var turnText, eventID string
			var body sql.RawBytes
			if err = rows.Scan(&last, &turnText, &eventID, &body); err != nil {
				return page, err
			}
			if entry.head == 0 {
				entry.head, entry.headID = last, eventID
			}
			turn, err := protocol.ParseTurnID(turnText)
			if err != nil {
				return page, err
			}
			var record canonical.Record
			if err = protocol.Decode(body, &record); err != nil {
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
				owner, err = readAssetLocation(ctx, db, session, turn)
				if err != nil {
					return page, err
				}
				if deps.bounded() {
					deps.turns[turn] = owner.fingerprint()
				}
				locations[turn] = owner
			}
			items, err := conversationAssetItems(ctx, db, record, owner.worktree, owner.root, deps)
			if err != nil {
				return page, err
			}
			if len(items) > 0 {
				page.Groups = append(page.Groups, protocol.ConversationAssetsGroup{Sequence: last, Turn: turn, Worktree: owner.worktree, TimestampMs: record.Created.UnixMilli(), Kind: kind, Items: items})
				if len(page.Groups) == int(query.Limit) {
					more = more || last > oldest
					break
				}
			}
		}
		if err = errors.Join(rows.Err(), rows.Close()); err != nil {
			return page, err
		}
	}
	if more {
		page.NextBefore = &last
	}
	if deps != nil {
		if more {
			entry.next = last
		}
		if prior != nil && (query.Before != nil || !more) {
			entry.deps.merge(prior.deps)
			if query.Before != nil || entry.head == 0 {
				entry.head, entry.headID = prior.head, prior.headID
			}
			if query.Before == nil {
				page.Delta = true
				entry.next = prior.next
			}
		}
		page.Checkpoint = reads.remember(entry)
	}
	return page, nil
}

const conversationAssetMembers = ` FROM conversation_events h CROSS JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND h.sequence<=?2 AND h.sequence>?3 ORDER BY h.sequence DESC`

func conversationAssetBoundary(ctx context.Context, db *sql.Conn, session protocol.SessionID, after, through uint64) (uint64, bool, error) {
	rows, err := db.QueryContext(ctx, `SELECT h.sequence,`+historyStoredBytes+conversationAssetMembers+` LIMIT 257`, session.String(), through, after)
	if err != nil {
		return 0, false, err
	}
	var last uint64
	var storedBytes int64
	count, more := 0, false
	for rows.Next() {
		var seq uint64
		var size int64
		if err = rows.Scan(&seq, &size); err != nil {
			rows.Close()
			return 0, false, err
		}
		// One oversized event must advance on its own. Read only lengths up to
		// the cutoff so a second body cannot cross the physical page budget.
		if count == 256 || count > 0 && size > (4<<20)-storedBytes {
			more = true
			break
		}
		count++
		storedBytes += size
		last = seq
	}
	return last, more, errors.Join(rows.Err(), rows.Close())
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
func conversationAssetItems(ctx context.Context, db *sql.Conn, record canonical.Record, worktree protocol.WorktreeID, root *string, deps *assetDependencies) ([]protocol.ConversationAssetsTarget, error) {
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
						if deps.bounded() {
							body, err := encode(attachment)
							if err != nil {
								return nil, err
							}
							deps.attachments[assetAttachmentKey{record.Turn, id}] = sha256.Sum256(body)
						}
					}
				case *canonical.ToolResponse:
					if path, ok := generatedImageAssetFile(value); ok {
						targets.file(path)
						break
					}
					for index, asset := range conversationAssetImages(value) {
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
		if !ok || !conversationAssetResultMayContain(response.Result.Value, `"sailry_content"`) {
			continue
		}
		// Only file and artifact blocks contribute targets here. Preserve the full
		// decoder for escaped discriminators and values supplied by binary slots.
		body := response.Result.Value
		if len(response.Result.Assets) == 0 && !bytes.Contains(body, []byte(`"file"`)) && !bytes.Contains(body, []byte(`"artifact"`)) && !bytes.Contains(body, []byte(`\u`)) {
			continue
		}
		hydrated, err := HydrateCanonicalPayload(ctx, db, response.Result)
		if err != nil {
			return nil, err
		}
		for _, slot := range response.Result.Assets {
			if !deps.bounded() {
				break
			}
			deps.blobs[slot.Asset.Digest] = true
		}
		raw := rawAgentTool{Presentation: protocol.ToolPresentationContent, Result: &hydrated, Format: response.Format}
		for index, asset := range conversationAssetImages(response) {
			if supportedAgentImage(asset) {
				raw.Images = append(raw.Images, protocol.ConversationImage{Index: uint64(index)})
			}
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

func conversationAssetResultMayContain(value json.RawMessage, key string) bool {
	value = bytes.TrimSpace(value)
	// Binary slots replace an existing value with a string or byte array. They
	// cannot introduce a key or turn a non-object into a result envelope.
	// A Unicode escape can spell a key without the literal marker.
	return len(value) > 0 && value[0] == '{' && (bytes.Contains(value, []byte(key)) || bytes.Contains(value, []byte(`\u`)))
}

func conversationAssetImages(response *canonical.ToolResponse) []canonical.AssetRef {
	if response.Format == canonical.JSON && len(response.Result.Assets) != 0 && !conversationAssetResultMayContain(response.Result.Value, `"inline_data"`) {
		attachments := *response
		attachments.Result.Assets = nil
		return agentImageAssets(&attachments)
	}
	return agentImageAssets(response)
}

func generatedImageAssetFile(response *canonical.ToolResponse) (string, bool) {
	if response.Name != "generate_image" || response.Status != canonical.ToolSuccess || response.Format != canonical.JSON || len(response.Attachments) != 1 || len(response.Result.Assets) != 0 {
		return "", false
	}
	asset := response.Attachments[0].Asset
	if !supportedAgentImage(asset) {
		return "", false
	}
	var result struct {
		Path      string `json:"path"`
		MediaType string `json:"media_type"`
		Size      uint64 `json:"size"`
	}
	if json.Unmarshal(response.Result.Value, &result) != nil || !validAssetRelative(result.Path) || result.MediaType != asset.Mime || result.Size != asset.Size {
		return "", false
	}
	// Native generation stores the same bytes in its file and preview attachment.
	// Prefer its literal workspace path so a later Markdown image publication
	// refers to one inventory target while canonical image access stays intact.
	return result.Path, true
}
