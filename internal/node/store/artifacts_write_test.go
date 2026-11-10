package store

import (
	"context"
	"database/sql"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCapturedArtifactPublication(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	owner := protocol.PluginReference{Name: "fixture-document", Digest: strings.Repeat("a", 64), SettingsRevision: 1}
	draft := protocol.ArtifactWrite{Kind: protocol.ArtifactKindDocument, Title: "Report", Html: `<p class="custom removed" onclick="bad()">Body<script>secret()</script><a href="https://example.invalid">Link</a></p><pre class="chart">{"type":"line","data":[{"label":"A","value":1}]}</pre>`}
	arguments, err := encode(draft)
	if err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(node, protocol.Command{Kind: "write_artifact", Data: &draft})
	captured := protocol.DefaultPluginContext()
	captured.Package, captured.Session, captured.Turn, captured.Worktree = owner, &session.ID, &turn, &session.Worktree
	request.Plugin = &captured
	approval := protocol.DefaultConversationApproval()
	approval.ID, approval.Session, approval.Turn = protocol.NewApprovalID(), session.ID, turn
	approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceUser
	operation := protocol.ToolOperationWriteArtifact
	binding := ApprovalBinding{Owner: owner, Tool: "write_document", Operation: &operation}
	var prepared ArtifactWrite
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		plugins, err := encode([]protocol.PluginReference{owner})
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE turns SET plugins=?2 WHERE id=?1", turn.String(), plugins); err != nil {
			return err
		}
		record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &canonical.ToolRequest{ID: "artifact-call", Name: binding.Tool, Arguments: arguments}})), now)
		record.Provenance.Author = "sailry"
		if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		approval.Entry = record.ID
		body, err := encode(approval)
		if err != nil {
			return err
		}
		bound, err := encode(binding)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'artifact-call',?3,?4,?5,1)", approval.ID.String(), turn.String(), body, bound, request.ID.String()); err != nil {
			return err
		}
		body, err = encode(request)
		if err != nil {
			return err
		}
		if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
			return err
		}
		prepared, err = CaptureArtifactWrite(ctx, db, request, []string{"custom", "chart"}, now)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(prepared.HTML, "secret") || strings.Contains(prepared.HTML, "onclick") || strings.Contains(prepared.HTML, "https:") || strings.Contains(prepared.HTML, "removed") || !strings.Contains(prepared.HTML, `class="custom"`) || !strings.Contains(prepared.HTML, `class="chart"`) || len(prepared.Warnings) == 0 {
		t.Fatalf("sanitized document: %s (%v)", prepared.HTML, prepared.Warnings)
	}
	profile, err := filepath.EvalSymlinks(storage.Profile)
	if err != nil {
		t.Fatal(err)
	}
	digest, err := files.WriteArtifactHTML(ctx, profile, prepared.Artifact, prepared.HTML)
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		output, err := PublishArtifactWrite(ctx, db, prepared, digest)
		if err != nil {
			return err
		}
		if output.Kind != "artifact_written" {
			return fmt.Errorf("unexpected publication output: %s", output.Kind)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	var read ArtifactRead
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		request := protocol.NewRequest(node, protocol.Command{Kind: "read_artifact", Data: &protocol.CommandReadArtifactPayload{ID: prepared.Artifact.ID, Session: &session.ID}})
		var err error
		read, err = CaptureArtifactRead(ctx, db, request)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	html, err := files.ReadArtifactHTML(ctx, profile, read.Content.Artifact, read.Digest)
	if err != nil || html != prepared.HTML {
		t.Fatalf("immutable artifact: %q %v", html, err)
	}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := PublishArtifactWrite(ctx, db, prepared, digest); err == nil {
			return fmt.Errorf("referenced artifact revision was rewritten")
		}
		modified := request
		copy := draft
		copy.Html = "<p>Changed</p>"
		modified.Command.Data = &copy
		if _, err := CaptureArtifactWrite(ctx, db, modified, nil, now); err == nil {
			return fmt.Errorf("artifact approval accepted different content")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, source := range []string{`<pre class="chart">{"type":"pie","data":[{"label":"A","value":-1}]}</pre>`, `<pre class="chart">{"type":"line","data":[{"label":"A","value":1}],"unknown":true}</pre>`} {
		html, warnings, err := files.PrepareArtifactHTML(protocol.ArtifactKindDocument, source, []string{"chart"})
		if err != nil || strings.Contains(html, `class="chart"`) || len(warnings) == 0 {
			t.Fatalf("invalid chart fallback: %s %v %v", html, warnings, err)
		}
	}
	preview := `<script>run()</script>`
	html, warnings, err := files.PrepareArtifactHTML(protocol.ArtifactKindPreview, preview, nil)
	if err != nil || html != preview || len(warnings) != 0 {
		t.Fatalf("opaque preview source changed: %s %v", html, err)
	}
}
