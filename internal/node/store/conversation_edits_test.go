package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestRepeatedEditsKeepSessionAndRequestIdentity(t *testing.T) {
	t.Parallel()
	storage, node, session, original, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: session.Worktree, Name: "input.txt", MediaType: "text/plain", Size: 5, Revision: fmt.Sprintf("%x", blake3.Sum256([]byte("input")))}}
	var reference protocol.PluginReference
	var originalBody []byte
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := InsertAttachment(ctx, db, node, attachment); err != nil {
			return err
		}
		if err := bindAgentAttachments(ctx, db, original, session.Worktree, []protocol.AttachmentID{attachment.ID}); err != nil {
			return err
		}
		info, err := InstallPlugin(ctx, db, pluginTestInfo("conversation-fixture", 'a'), 0, true)
		if err != nil {
			return err
		}
		reference = plugins.PackageReference(info)
		if _, err = WritePluginConversationValue(ctx, db, reference, session.ID, &original, "checkpoint", json.RawMessage(`{"step":1}`), 0); err != nil {
			return err
		}
		user := canonical.NewRecord(original, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "Original input"))), now)
		user.Provenance.Author = "user"
		if _, _, err = AppendCanonical(ctx, db, session.ID, user); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, original, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		return db.QueryRowContext(ctx, "SELECT body FROM agent_events WHERE turn=?1", original.String()).Scan(&originalBody)
	}); err != nil {
		t.Fatal(err)
	}
	sessions := schemaInteger(t, storage.Node, "SELECT count(*) FROM sessions")
	turns := schemaInteger(t, storage.Node, "SELECT count(*) FROM turns")
	head := original
	for index := range 2 {
		message := protocol.DefaultConversationInput()
		message.Text = fmt.Sprintf("Edited input %d", index)
		message.Attachments = []protocol.AttachmentID{attachment.ID}
		var request protocol.Request
		var body []byte
		var replaced protocol.OutputTurnReplacedPayload
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			revision, err := historyRevision(ctx, db, session.ID)
			if err != nil {
				return err
			}
			request = protocol.NewRequest(node, protocol.Command{Kind: "replace_turn", Data: &protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: head, ExpectedHead: head, ExpectedRevision: session.Revision, ExpectedHistoryRevision: revision, Message: message}})
			body, err = encode(request)
			if err != nil {
				return err
			}
			if previous, err := admission.Admit(ctx, db, node, request, body); err != nil || previous != nil {
				return fmt.Errorf("unexpected previous edit admission: %v", err)
			}
			output, events, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
			if err != nil {
				return err
			}
			replaced = output.Data.(protocol.OutputTurnReplacedPayload)
			if replaced.Turn.Session != session.ID || replaced.History == nil || replaced.History.Backup != nil || replaced.History.Session != session.ID || replaced.History.Revision != revision+1 {
				return errors.New("edit changed session identity or failed to advance history")
			}
			if len(events) != 1 || events[0].Kind != "session_rewound" {
				return errors.New("edit did not publish one history change")
			}
			changed := events[0].Data.(protocol.EventSessionRewoundPayload)
			if changed.Session == nil || changed.Session.ID != session.ID || changed.Backup != nil {
				return errors.New("edit published another sidebar session")
			}
			value, err := ReadPluginConversationValue(ctx, db, reference, session.ID, "checkpoint")
			if err != nil || value.Present || !value.Restored {
				return fmt.Errorf("edit did not restore plugin state: %+v, %v", value, err)
			}
			if _, err = GetTurnAttachment(ctx, db, replaced.Turn.ID, attachment.ID); err != nil {
				return err
			}
			return admission.Complete(ctx, db, node, request.ID, link.Response{Output: output})
		}); err != nil {
			t.Fatal(err)
		}
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			previous, err := admission.Admit(ctx, db, node, request, body)
			if err != nil {
				return err
			}
			if previous == nil || previous.Fault != nil || previous.Output.Kind != "turn_replaced" {
				return errors.New("edit replay did not recover its completed admission")
			}
			replayed := previous.Output.Data.(*protocol.OutputTurnReplacedPayload)
			if replayed.Turn.ID != replaced.Turn.ID || replayed.Turn.Request != request.ID || replayed.History == nil || replayed.History.Backup != nil {
				return errors.New("edit replay changed its outcome")
			}
			invocation, _, err := ClaimNext(ctx, db, now)
			if err != nil || invocation == nil || invocation.Run.Turn != replaced.Turn.ID {
				return fmt.Errorf("replacement was not claimed: %v", err)
			}
			user := canonical.NewRecord(replaced.Turn.ID, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, message.Text))), now)
			user.Provenance.Author = "user"
			if _, _, err = AppendCanonical(ctx, db, session.ID, user); err != nil {
				return err
			}
			if _, _, err = FinishTurn(ctx, db, replaced.Turn.ID, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			var retained []byte
			if err = db.QueryRowContext(ctx, "SELECT body FROM agent_events WHERE turn=?1", original.String()).Scan(&retained); err != nil {
				return err
			}
			if !bytes.Equal(retained, originalBody) {
				return errors.New("edit changed the original canonical record")
			}
			if _, err = GetTurnAttachment(ctx, db, original, attachment.ID); err != nil {
				return err
			}
			page, err := ReadConversation(ctx, db, session.ID, nil, 20)
			if err == nil && (len(page.Page.Runs) != 1 || page.Page.Runs[0].Turn != replaced.Turn.ID) {
				return errors.New("edited session retained superseded visible turns")
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if count := schemaInteger(t, storage.Node, "SELECT count(*) FROM sessions"); count != sessions {
			t.Fatalf("edit %d created another session: %d -> %d", index, sessions, count)
		}
		if count := schemaInteger(t, storage.Node, "SELECT count(*) FROM turns"); count != turns+int64(index)+1 {
			t.Fatalf("edit %d or its replay admitted duplicate turns: %d", index, count)
		}
		head = replaced.Turn.ID
	}
}
