package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type ArtifactWrite struct {
	Artifact protocol.ArtifactSummary
	Previous *protocol.ArtifactSummary
	HTML     string
	Warnings []string
	Digest   string
	Request  protocol.Request
}

func authorizeArtifactWrite(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.ArtifactWrite, error) {
	draft, ok := filePayload[protocol.ArtifactWrite](request.Command.Data)
	if request.Command.Kind != "write_artifact" || !ok {
		return draft, invalid("artifact write command required")
	}
	captured := request.Plugin
	if captured == nil || captured.Turn == nil || captured.Session == nil || captured.Worktree == nil || captured.Invocation != nil {
		return draft, fault(protocol.ErrorCodePermissionDenied, "artifact writes require an admitted plugin turn")
	}
	owner, err := ReadSession(ctx, db, *captured.Session)
	if err != nil {
		return draft, err
	}
	if owner.Delegation != nil && *owner.Delegation != nil {
		return draft, fault(protocol.ErrorCodePermissionDenied, "delegated sessions cannot write artifacts")
	}
	run, err := visibleAgentRun(ctx, db, *captured.Session, *captured.Turn)
	if err != nil {
		return draft, err
	}
	if run.Origin != nil || run.Status != protocol.ConversationStatusRunning || run.Worktree != *captured.Worktree {
		return draft, fault(protocol.ErrorCodeCancelled, "artifact turn is no longer active")
	}
	turn, err := readTurnCapture(ctx, db, *captured.Turn)
	if err != nil {
		return draft, err
	}
	if !containsPlugin(turn.Plugins, captured.Package) {
		return draft, fault(protocol.ErrorCodePermissionDenied, "artifact package is outside the captured tool catalog")
	}
	var body, bound []byte
	err = db.QueryRowContext(ctx, "SELECT body,binding FROM agent_approvals WHERE request=?1 AND turn=?2 AND claimed=1", request.ID.String(), captured.Turn.String()).Scan(&body, &bound)
	if errors.Is(err, sql.ErrNoRows) {
		return draft, fault(protocol.ErrorCodePermissionDenied, "artifact write has no claimed tool approval")
	}
	if err != nil {
		return draft, err
	}
	var approval protocol.ConversationApproval
	var binding *ApprovalBinding
	if err = json.Unmarshal(body, &approval); err != nil {
		return draft, err
	}
	if err = json.Unmarshal(bound, &binding); err != nil {
		return draft, err
	}
	if approval.State != protocol.ConversationApprovalStateApproved || binding == nil || binding.Operation == nil || *binding.Operation != protocol.ToolOperationWriteArtifact || !containsPlugin([]protocol.PluginReference{binding.Owner}, captured.Package) {
		return draft, fault(protocol.ErrorCodePermissionDenied, "artifact write is outside its captured approval")
	}
	call, err := approvalRequest(ctx, db, approval)
	if err != nil {
		return draft, err
	}
	var expected protocol.ArtifactWrite
	if call.Name != binding.Tool || json.Unmarshal(call.Arguments, &expected) != nil {
		return draft, fault(protocol.ErrorCodePermissionDenied, "artifact write differs from its canonical tool call")
	}
	a, err := encode(expected)
	if err != nil {
		return draft, err
	}
	b, err := encode(draft)
	if err != nil {
		return draft, err
	}
	if !bytes.Equal(a, b) {
		return draft, fault(protocol.ErrorCodePermissionDenied, "artifact write differs from its canonical tool call")
	}
	return draft, nil
}
func CaptureArtifactWrite(ctx context.Context, db *sql.Conn, request protocol.Request, allowedClasses []string, now time.Time) (ArtifactWrite, error) {
	out := ArtifactWrite{Request: request}
	draft, err := authorizeArtifactWrite(ctx, db, request)
	if err != nil {
		return out, err
	}
	label := func(value string, empty bool) bool {
		return (empty || strings.TrimSpace(value) != "") && strings.IndexFunc(value, unicode.IsControl) < 0
	}
	if !label(draft.Title, false) || !label(draft.Summary, true) || (draft.ID == nil) != (draft.ExpectedRevision == nil) {
		return out, invalid("invalid artifact title, summary or expected revision")
	}
	if draft.ID != nil {
		previous, err := requiredArtifact(ctx, db, *draft.ID)
		if err != nil {
			return out, err
		}
		if previous.Session != *request.Plugin.Session || previous.Package.Name != request.Plugin.Package.Name {
			return out, fault(protocol.ErrorCodePermissionDenied, "artifact belongs to another session or package")
		}
		if previous.Revision != *draft.ExpectedRevision {
			return out, fault(protocol.ErrorCodeRevisionConflict, "artifact revision changed; read it before editing")
		}
		if previous.Revision == math.MaxUint32 {
			return out, invalid("artifact revision exhausted")
		}
		out.Previous = &previous
	}
	out.HTML, out.Warnings, err = files.PrepareArtifactHTML(draft.Kind, draft.Html, allowedClasses)
	if err != nil {
		return out, err
	}
	timestamp := now.UnixMilli()
	if timestamp < 0 {
		return out, invalid("artifact creation time is invalid")
	}
	out.Artifact = protocol.ArtifactSummary{ID: protocol.NewArtifactID(), Kind: draft.Kind, Session: *request.Plugin.Session, Turn: *request.Plugin.Turn, Package: request.Plugin.Package, Title: draft.Title, Summary: draft.Summary, Revision: 1, Size: uint64(len(out.HTML)), CreatedAt: timestamp, UpdatedAt: timestamp}
	if out.Previous != nil {
		out.Artifact.ID = out.Previous.ID
		out.Artifact.Revision = out.Previous.Revision + 1
		out.Artifact.CreatedAt = out.Previous.CreatedAt
		out.Artifact.UpdatedAt = max(timestamp, out.Previous.CreatedAt)
	}
	out.Digest = fmt.Sprintf("%x", blake3.Sum256([]byte(out.HTML)))
	var referenced bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM artifact_revisions WHERE artifact=?1 AND revision=?2)", out.Artifact.ID.String(), out.Artifact.Revision).Scan(&referenced); err != nil {
		return out, err
	}
	if referenced {
		return out, fault(protocol.ErrorCodeRevisionConflict, "artifact revision already exists")
	}
	output := protocol.Output{Kind: "artifact_written", Data: protocol.OutputArtifactWrittenPayload{Artifact: out.Artifact, Warnings: out.Warnings}}
	content := protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "artifact", Data: protocol.ToolContentBlockArtifactPayload{Artifact: out.Artifact}}}}
	metadata, err := encode([]any{output, content})
	if err != nil {
		return out, err
	}
	if len(metadata) > artifactPageBytes-(512<<10)-(256<<10) {
		return out, invalid("artifact metadata exceeds the response budget")
	}
	return out, nil
}
func PublishArtifactWrite(ctx context.Context, db *sql.Conn, capture ArtifactWrite, digest string) (protocol.Output, error) {
	if digest != capture.Digest {
		return protocol.Output{}, fault(protocol.ErrorCodeOutcomeUnknown, "artifact file publication could not be confirmed")
	}
	if _, err := authorizeArtifactWrite(ctx, db, capture.Request); err != nil {
		return protocol.Output{}, err
	}
	var rowid uint64
	if capture.Previous != nil {
		current, err := requiredArtifact(ctx, db, capture.Artifact.ID)
		if err != nil {
			return protocol.Output{}, err
		}
		a, err := encode(current)
		if err != nil {
			return protocol.Output{}, err
		}
		b, err := encode(*capture.Previous)
		if err != nil {
			return protocol.Output{}, err
		}
		if !bytes.Equal(a, b) {
			return protocol.Output{}, fault(protocol.ErrorCodeRevisionConflict, "artifact revision changed during publication")
		}
		if err = db.QueryRowContext(ctx, "SELECT rowid FROM artifacts WHERE id=?1", capture.Artifact.ID.String()).Scan(&rowid); err != nil {
			return protocol.Output{}, err
		}
	} else {
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM artifacts WHERE id=?1)", capture.Artifact.ID.String()).Scan(&exists); err != nil {
			return protocol.Output{}, err
		}
		if exists {
			return protocol.Output{}, fault(protocol.ErrorCodeRevisionConflict, "artifact already exists")
		}
		var err error
		rowid, err = NextCounter(ctx, db, CounterArtifacts)
		if err != nil {
			return protocol.Output{}, err
		}
	}
	var exists bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM artifact_revisions WHERE artifact=?1 AND revision=?2)", capture.Artifact.ID.String(), capture.Artifact.Revision).Scan(&exists); err != nil {
		return protocol.Output{}, err
	}
	if exists {
		return protocol.Output{}, fault(protocol.ErrorCodeRevisionConflict, "artifact revision already exists")
	}
	body, err := encode(capture.Artifact)
	if err != nil {
		return protocol.Output{}, err
	}
	warnings, err := encode(capture.Warnings)
	if err != nil {
		return protocol.Output{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO artifacts(rowid,id,session,body) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET body=excluded.body", rowid, capture.Artifact.ID.String(), capture.Artifact.Session.String(), body)
	if err != nil {
		return protocol.Output{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO artifact_revisions(artifact,revision,body,digest,warnings) VALUES(?1,?2,?3,?4,?5)", capture.Artifact.ID.String(), capture.Artifact.Revision, body, digest, warnings)
	return protocol.Output{Kind: "artifact_written", Data: protocol.OutputArtifactWrittenPayload{Artifact: capture.Artifact, Warnings: capture.Warnings}}, err
}
