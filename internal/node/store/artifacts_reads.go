package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const artifactPageBytes = protocol.MAX_FRAME_BYTES / 2

// ArtifactRead captures immutable metadata for a resource-worker read. It owns no
// database connection and must be rechecked against the session after IO.
type ArtifactRead struct {
	Content protocol.ArtifactContent
	Digest  string
}

func requiredArtifact(ctx context.Context, db *sql.Conn, id protocol.ArtifactID) (protocol.ArtifactSummary, error) {
	value, err := readBody[protocol.ArtifactSummary](ctx, db, "SELECT body FROM artifacts WHERE id=?1", id.String())
	if err != nil {
		return protocol.ArtifactSummary{}, err
	}
	if value == nil {
		return protocol.ArtifactSummary{}, fault(protocol.ErrorCodeNotFound, "artifact does not exist")
	}
	if !validArtifactSummary(*value) {
		return protocol.ArtifactSummary{}, errors.New("invalid artifact metadata")
	}
	return *value, nil
}
func artifactScope(ctx context.Context, db *sql.Conn, request protocol.Request, id protocol.ArtifactID, session *protocol.SessionID) (protocol.SessionID, error) {
	if request.Plugin != nil && request.Plugin.Session != nil {
		if session != nil && *session != *request.Plugin.Session {
			return protocol.SessionID{}, fault(protocol.ErrorCodePermissionDenied, "artifact read is outside the captured session")
		}
		session = request.Plugin.Session
	}
	if session == nil {
		latest, err := requiredArtifact(ctx, db, id)
		if err != nil {
			return protocol.SessionID{}, err
		}
		session = &latest.Session
	}
	if err := requireSession(ctx, db, *session); err != nil {
		return protocol.SessionID{}, err
	}
	return *session, nil
}
func artifactPublished(ctx context.Context, db *sql.Conn, session protocol.SessionID, artifact protocol.ArtifactSummary) (bool, error) {
	records, err := readBodies[canonical.Record](ctx, db, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2", session.String(), artifact.Turn.String())
	if err != nil {
		return false, err
	}
	want, err := json.Marshal(artifact)
	if err != nil {
		return false, err
	}
	for _, record := range records {
		message := record.Message()
		if message == nil {
			continue
		}
		for _, block := range message.Content {
			response, ok := block.Data.(*canonical.ToolResponse)
			if !ok {
				continue
			}
			hydrated, err := HydrateCanonicalPayload(ctx, db, response.Result)
			if err != nil {
				return false, err
			}
			raw := rawAgentTool{Presentation: protocol.ToolPresentationContent, Result: &hydrated, Format: response.Format}
			content := raw.content()
			if content == nil {
				continue
			}
			for _, part := range content.Blocks {
				if part.Kind != "artifact" {
					continue
				}
				value, ok := filePayload[protocol.ToolContentBlockArtifactPayload](part.Data)
				if !ok {
					continue
				}
				body, err := json.Marshal(value.Artifact)
				if err != nil {
					return false, err
				}
				if bytes.Equal(body, want) {
					return true, nil
				}
			}
		}
	}
	return false, nil
}
func artifactVisible(ctx context.Context, db *sql.Conn, session protocol.SessionID, latest, version protocol.ArtifactSummary) (bool, error) {
	if !validArtifactSummary(version) || version.ID != latest.ID || version.Session != latest.Session || version.CreatedAt != latest.CreatedAt || version.Package.Name != latest.Package.Name || version.Revision > latest.Revision || version.Revision == 0 {
		return false, errors.New("invalid stored artifact revision")
	}
	if version.Session == session {
		return true, nil
	}
	return artifactPublished(ctx, db, session, version)
}
func artifactLimit(limit uint16) error {
	if limit < 1 || limit > 64 {
		return invalid("artifact page limit must be between 1 and 64")
	}
	return nil
}
func ListArtifacts(ctx context.Context, db *sql.Conn, session protocol.SessionID, after *protocol.ArtifactID, limit uint16) (protocol.ArtifactPage, error) {
	out := protocol.DefaultArtifactPage()
	if err := requireSession(ctx, db, session); err != nil {
		return out, err
	}
	if err := artifactLimit(limit); err != nil {
		return out, err
	}
	boundary := int64(math.MaxInt64)
	if after != nil {
		var owner string
		err := db.QueryRowContext(ctx, "SELECT rowid,session FROM artifacts WHERE id=?1", after.String()).Scan(&boundary, &owner)
		if errors.Is(err, sql.ErrNoRows) || err == nil && owner != session.String() {
			return out, invalid("artifact cursor belongs to another session")
		}
		if err != nil {
			return out, err
		}
	}
	values, err := readBodies[protocol.ArtifactSummary](ctx, db, "SELECT body FROM artifacts WHERE session=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT ?3", session.String(), boundary, int(limit)+1)
	if err != nil {
		return out, err
	}
	budget := 128
	more := false
	for _, value := range values {
		body, err := encode(value)
		if err != nil {
			return out, err
		}
		if len(out.Items) == int(limit) || budget+len(body)+1 > artifactPageBytes {
			if len(out.Items) == 0 {
				return out, invalid("artifact metadata exceeds the response budget")
			}
			more = true
			break
		}
		budget += len(body) + 1
		out.Items = append(out.Items, value)
	}
	if more {
		out.NextAfter = &out.Items[len(out.Items)-1].ID
	}
	return out, nil
}
func ListArtifactRevisions(ctx context.Context, db *sql.Conn, id protocol.ArtifactID, before *uint32, limit uint16, session protocol.SessionID) (protocol.ArtifactRevisionPage, error) {
	out := protocol.DefaultArtifactRevisionPage()
	if err := artifactLimit(limit); err != nil {
		return out, err
	}
	latest, err := requiredArtifact(ctx, db, id)
	if err != nil {
		return out, err
	}
	boundary := int64(math.MaxInt64)
	if before != nil {
		boundary = int64(*before)
	}
	values, err := readBodies[protocol.ArtifactSummary](ctx, db, "SELECT body FROM artifact_revisions WHERE artifact=?1 AND revision<?2 ORDER BY revision DESC", id.String(), boundary)
	if err != nil {
		return out, err
	}
	budget := 128
	more := false
	for _, value := range values {
		visible, err := artifactVisible(ctx, db, session, latest, value)
		if err != nil {
			return out, err
		}
		if !visible {
			continue
		}
		revision := protocol.ArtifactRevision{ID: value.ID, Kind: value.Kind, Revision: value.Revision, Turn: value.Turn, Title: value.Title, Summary: value.Summary, Size: value.Size, CreatedAt: value.UpdatedAt}
		body, err := encode(revision)
		if err != nil {
			return out, err
		}
		if len(out.Items) == int(limit) || budget+len(body)+1 > artifactPageBytes {
			if len(out.Items) == 0 {
				return out, invalid("artifact metadata exceeds the response budget")
			}
			more = true
			break
		}
		budget += len(body) + 1
		out.Items = append(out.Items, revision)
	}
	if latest.Session != session && len(out.Items) == 0 && before == nil {
		return out, fault(protocol.ErrorCodePermissionDenied, "artifact has no publication in this conversation")
	}
	if more {
		out.NextBefore = &out.Items[len(out.Items)-1].Revision
	}
	return out, nil
}
func CaptureArtifactRead(ctx context.Context, db *sql.Conn, request protocol.Request) (ArtifactRead, error) {
	out := ArtifactRead{}
	if request.Command.Kind != "read_artifact" {
		return out, invalid("artifact read command required")
	}
	input := request.Command.Data.(*protocol.CommandReadArtifactPayload)
	session, err := artifactScope(ctx, db, request, input.ID, input.Session)
	if err != nil {
		return out, err
	}
	latest, err := requiredArtifact(ctx, db, input.ID)
	if err != nil {
		return out, err
	}
	visible := latest.Revision
	if latest.Session != session {
		page, err := ListArtifactRevisions(ctx, db, input.ID, nil, 1, session)
		if err != nil {
			return out, err
		}
		if len(page.Items) == 0 {
			return out, fault(protocol.ErrorCodePermissionDenied, "artifact has no publication in this conversation")
		}
		visible = page.Items[0].Revision
	}
	revision := visible
	if input.Revision != nil {
		revision = *input.Revision
	}
	var body, warnings []byte
	err = db.QueryRowContext(ctx, "SELECT body,digest,warnings FROM artifact_revisions WHERE artifact=?1 AND revision=?2", input.ID.String(), revision).Scan(&body, &out.Digest, &warnings)
	if errors.Is(err, sql.ErrNoRows) {
		return out, fault(protocol.ErrorCodeNotFound, "artifact revision does not exist")
	}
	if err != nil {
		return out, err
	}
	if err = json.Unmarshal(body, &out.Content.Artifact); err != nil {
		return out, err
	}
	if out.Content.Artifact.Revision != revision {
		return out, errors.New("invalid stored artifact revision")
	}
	allowed, err := artifactVisible(ctx, db, session, latest, out.Content.Artifact)
	if err != nil {
		return out, err
	}
	if !allowed {
		return out, fault(protocol.ErrorCodePermissionDenied, "artifact revision is outside the conversation history")
	}
	out.Content.LatestRevision = visible
	err = json.Unmarshal(warnings, &out.Content.Warnings)
	return out, err
}
func ReadArtifactCommand(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.Output, error) {
	switch request.Command.Kind {
	case "list_artifacts":
		v := request.Command.Data.(*protocol.CommandListArtifactsPayload)
		value, err := ListArtifacts(ctx, db, v.Session, v.After, v.Limit)
		return protocol.Output{Kind: "artifacts", Data: value}, err
	case "list_artifact_revisions":
		v := request.Command.Data.(*protocol.CommandListArtifactRevisionsPayload)
		session, err := artifactScope(ctx, db, request, v.ID, v.Session)
		if err != nil {
			return protocol.Output{}, err
		}
		value, err := ListArtifactRevisions(ctx, db, v.ID, v.Before, v.Limit, session)
		return protocol.Output{Kind: "artifact_revisions", Data: value}, err
	}
	return protocol.Output{}, invalid("artifact content requires a captured resource read")
}

func validArtifactSummary(value protocol.ArtifactSummary) bool {
	return value.Revision > 0 && value.Size > 0 && value.Size <= 512<<10 && strings.TrimSpace(value.Title) != "" && strings.IndexFunc(value.Title, unicode.IsControl) < 0 && strings.IndexFunc(value.Summary, unicode.IsControl) < 0 && value.CreatedAt <= value.UpdatedAt
}
