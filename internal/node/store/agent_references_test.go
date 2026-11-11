package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func agentHistoryFixture(t *testing.T, mode protocol.WorkMode) (*Store, protocol.NodeID, protocol.Session, protocol.TurnID, time.Time) {
	t.Helper()
	profile, node, storage := schemaFixture(t)
	ctx := context.Background()
	now := time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC)
	var session protocol.Session
	var turn protocol.TurnID
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := protocol.DefaultConversationProvider()
		provider.ID = protocol.NewProviderID()
		provider.Revision = 1
		provider.Name = "Fixture"
		provider.Api = protocol.ConversationModelApiChatCompletions
		provider.Authentication = protocol.AuthenticationApiKey
		provider.Endpoint = "http://fixture.invalid/v1"
		provider.Enabled = true
		provider.DefaultModel = "fixture"
		model := protocol.DefaultConversationModel()
		model.ID = "fixture"
		model.Context = 32000
		model.Output = 8000
		model.Tools = true
		provider.Models = []protocol.ConversationModel{model}
		saved := providerConfiguration{ID: provider.ID, Provider: &provider, Listed: true, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
		body, err := encode(saved)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body); err != nil {
			return err
		}
		config := protocol.DefaultSessionConfig()
		config.Provider = provider.ID
		config.Model = "fixture"
		config.Mode = mode
		config.Permission = protocol.PermissionAsk
		output, _, err := createSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err != nil {
			return err
		}
		session = output.Data.(protocol.Session)
		request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Fixture input"}}})
		body, err = encode(request)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
			return err
		}
		capture, err := admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
		if err != nil {
			return err
		}
		turn = capture.ID
		inv, _, err := ClaimNext(ctx, db, now)
		if err != nil {
			return err
		}
		if inv == nil || inv.Run.Turn != turn {
			return errors.New("fixture run was not claimed")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return storage, node, session, turn, now
}

// Over 1 s only beside the package's other parallel tests: each opens its own synced store.
func TestAgentHistoryAndReview(t *testing.T) {
	t.Parallel()
	t.Run("canonical card authority", testPluginCardAuthority)
	t.Run("native task admission", testTaskAdmission)
	t.Run("retained command stream keeps invocation and canonical revision", func(t *testing.T) {
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		command := protocol.NewRequestID()
		var reference, unrelated protocol.ToolOutputOutputReference
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			body, err := encode(map[string]any{"kind": "command_result", "data": map[string]any{"stdout": map[string]any{"text": "captured"}}, "sailry_command": map[string]any{"id": command}})
			if err != nil {
				return err
			}
			reference = protocol.ToolOutputOutputReference{Call: "stream-call", Revision: fmt.Sprintf("%x", blake3.Sum256(body))}
			response := canonical.ToolResponse{ID: reference.Call, Name: "fixture-command", Format: canonical.JSON, Status: canonical.ToolSuccess, Revision: reference.Revision, Result: canonical.Payload{Value: body}}
			record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response})), now)
			record.Provenance.Author = "tool"
			if _, _, err = AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
			body, err = encode(map[string]any{"id": command, "business": map[string]any{"info": map[string]any{"id": command}}})
			if err != nil {
				return err
			}
			unrelated = protocol.ToolOutputOutputReference{Call: "business-call", Revision: fmt.Sprintf("%x", blake3.Sum256(body))}
			response = canonical.ToolResponse{ID: unrelated.Call, Name: "fixture-business", Format: canonical.JSON, Status: canonical.ToolSuccess, Revision: unrelated.Revision, Result: canonical.Payload{Value: body}}
			record = canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response})), now)
			record.Provenance.Author = "tool"
			_, _, err = AppendCanonical(ctx, db, session.ID, record)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		err = storage.Node.Read(ctx, func(db *sql.Conn) error {
			input := protocol.CommandReadToolOutputPayload{Turn: turn, Output: reference, Limit: 100}
			owner, id, err := ResolveToolCommandStream(ctx, db, node, input)
			if err != nil {
				return err
			}
			if owner != session.ID || id != command {
				return errors.New("retained command owner changed")
			}
			wrong := node
			wrong[0]++
			if _, _, err = ResolveToolCommandStream(ctx, db, wrong, input); err == nil {
				return errors.New("another invocation read retained output")
			}
			input.Output.Revision = strings.Repeat("0", 64)
			if _, _, err = ResolveToolCommandStream(ctx, db, node, input); err == nil {
				return errors.New("stale canonical output revision was accepted")
			}
			input.Output = unrelated
			if _, _, err = ResolveToolCommandStream(ctx, db, node, input); err == nil {
				return errors.New("unrelated business identifiers became retained command authority")
			}
			input.Turn = protocol.NewTurnID()
			var absent *protocol.Fault
			if _, _, err = ResolveToolCommandStream(ctx, db, node, input); !errors.As(err, &absent) || absent.Code != protocol.ErrorCodeNotFound {
				return fmt.Errorf("unknown turn stream: %v", err)
			}
			if _, err = ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_tool_output", Data: &input}); !errors.As(err, &absent) || absent.Code != protocol.ErrorCodeNotFound {
				return fmt.Errorf("unknown turn output: %v", err)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("source citations validate Unicode ranges and preserve grounding", func(t *testing.T) {
		uri, title := "https://example.com/source", "Public source"
		start, end, tooFar, negative := int64(0), int64(3), int64(4), int64(-1)
		message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "结果🙂"))
		message.Sources = canonical.Sources{Citations: []canonical.Citation{{URI: &uri, Title: &title, Start: &start, End: &end}, {URI: &uri, Start: &start, End: &tooFar}, {URI: &uri, Start: &negative, End: &end}, {URI: ptr("file:///private/source"), Start: &start, End: &end}}, Grounding: []canonical.Grounding{{URI: "https://grounded.example/source", Title: &title}, {URI: "file:///private/source"}}, SearchSuggestions: ptr("<div>Provider suggestions</div>")}
		record := canonical.NewRecord(protocol.NewTurnID(), "message", &message, time.Unix(1, 0))
		record.Provenance.Author = "assistant"
		entry, err := CanonicalEntry(record, 1)
		if err != nil {
			t.Fatal(err)
		}
		if len(entry.Citations) != 4 || entry.Citations[0].End == nil || *entry.Citations[0].End != 3 || entry.Citations[1].End != nil || entry.Citations[2].Start != nil || entry.Citations[3].Uri != "https://grounded.example/source" || entry.SearchSuggestions == nil {
			t.Fatal(entry)
		}
	})

	t.Run("scope revision and explicit truncation", func(t *testing.T) {
		t.Parallel()
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := context.Background()
		source := AgentHistorySource{Caller: node, Session: session.ID, Turn: turn}
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, strings.Repeat("Long fixture ", 2000)))), now)
			record.Provenance.Author = "user"
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
			args, _ := encode(map[string]any{"session": session.ID})
			body, err := ReadAgentReference(ctx, db, node, source, "read_session", args)
			if err != nil {
				return err
			}
			if len(body) > 40*1024 || !strings.Contains(string(body), `"truncated":true`) || !strings.Contains(string(body), `"truncated_entries":["`) {
				return fmt.Errorf("history omissions were not explicit: %s", body)
			}
			for _, bad := range []json.RawMessage{json.RawMessage(fmt.Sprintf(`{"session":%q,"turn":%q}`, session.ID.String(), turn.String())), json.RawMessage(fmt.Sprintf(`{"session":%q,"revision":999}`, session.ID.String()))} {
				if _, err = ReadAgentReference(ctx, db, node, source, "read_session", bad); err == nil {
					return errors.New("invalid history continuation was accepted")
				}
			}
			wrong := source
			wrong.Caller[0]++
			if _, err = ReadAgentReference(ctx, db, node, wrong, "read_session", args); err == nil {
				return errors.New("other invocation read history")
			}
			other, _, err := createSession(ctx, db, node, "", protocol.CommandCreateSessionPayload{Config: &session.Config})
			if err != nil {
				return err
			}
			target := other.Data.(protocol.Session)
			project := protocol.NewProjectID()
			appearance, _ := encode(protocol.DefaultProjectsAppearance())
			if _, err = db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance) VALUES(?1,'Other','/fixture-other',?2)", project.String(), appearance); err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "UPDATE sessions SET project=?2 WHERE id=?1", target.ID.String(), project.String()); err != nil {
				return err
			}
			args, _ = encode(map[string]any{"session": target.ID})
			if _, err = ReadAgentReference(ctx, db, node, source, "read_session", args); err == nil {
				return errors.New("cross-project history was accepted")
			}
			var latest protocol.Session
			for i := 0; i < 258; i++ {
				output, _, e := createSession(ctx, db, node, "", protocol.CommandCreateSessionPayload{Config: &session.Config})
				if e != nil {
					return e
				}
				latest = output.Data.(protocol.Session)
			}
			body, err = ReadAgentReference(ctx, db, node, source, "list_sessions", json.RawMessage(`{"text":"never-occurs"}`))
			if err != nil {
				return err
			}
			var directory agentDirectoryPage
			if err = json.Unmarshal(body, &directory); err != nil {
				return err
			}
			if len(directory.Sessions) != 0 || directory.NextBefore == nil {
				return errors.New("empty filtered directory lost its bounded continuation")
			}
			if strings.Contains(string(body), `"config"`) || strings.Contains(string(body), `"credential"`) {
				return errors.New("directory leaked execution configuration")
			}
			if _, _, err = renameSession(ctx, db, protocol.CommandRenameSessionPayload{Session: latest.ID, ExpectedRevision: latest.Revision, Title: "Changed title"}); err != nil {
				return err
			}
			args, _ = encode(map[string]any{"revision": directory.Revision})
			if _, err = ReadAgentReference(ctx, db, node, source, "list_sessions", args); err == nil {
				return errors.New("changed directory revision was accepted")
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	t.Run("completed review remains actionable and durable", func(t *testing.T) {
		t.Parallel()
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModePlan)
		ctx := context.Background()
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Completed plan"))), now)
			record.Provenance.Author = "assistant"
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			entry, question, _, err := PublishPlanReview(ctx, db, turn, now)
			if err != nil {
				return err
			}
			if entry == nil || question == nil || len(entry.Parts) != 1 || question.State.Kind != "pending" {
				return errors.New("successful plan did not publish durable review")
			}
			spec, err := questionSpec(ctx, db, *question)
			if err != nil || spec.Input.Kind != "plan" {
				return errors.New("review lost its plan input")
			}
			if _, _, err = ExpireQuestion(ctx, db, question.ID); err == nil {
				return errors.New("deferred plan acquired timeout consent")
			}
			request := protocol.NewRequest(node, protocol.Command{Kind: "resolve_question", Data: &protocol.CommandResolveQuestionPayload{Session: session.ID, Question: question.ID, Response: protocol.ConversationQuestionResponse{Kind: "answer", Data: &protocol.ConversationQuestionAnswer{Kind: "text", Data: ptr("Refine the plan")}}}})
			output, _, err := resolveQuestion(ctx, db, node, node, request)
			if err != nil {
				return err
			}
			changes, _, err := ApplyReviewCommand(ctx, db, request, output, now)
			if err != nil {
				return err
			}
			if len(changes) != 1 || changes[0].Change.Kind != "entry" {
				return errors.New("review result was not canonical")
			}
			again, _, err := ApplyReviewCommand(ctx, db, request, output, now)
			if err != nil {
				return err
			}
			if len(again) != 0 {
				return errors.New("repeated answer duplicated canonical result")
			}
			saved, err := ReadQuestion(ctx, db, question.ID)
			if err != nil {
				return err
			}
			if saved.State.Kind != "answered" {
				return errors.New("completed review did not settle")
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
	for _, name := range []string{"hidden note after the answer keeps the review", "empty answer has no review"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModePlan)
			ctx := context.Background()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				text := "Completed plan"
				if name == "empty answer has no review" {
					text = ""
				}
				answer := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, text))), now)
				answer.Provenance.Author = "assistant"
				if _, _, err := AppendCanonical(ctx, db, session.ID, answer); err != nil {
					return err
				}
				note := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "Subagent was cancelled before it finished."))), now)
				note.Provenance.Author = "user"
				note.Metadata.Runtime = &canonical.Runtime{AgentVisible: true}
				if _, _, err := AppendCanonical(ctx, db, session.ID, note); err != nil {
					return err
				}
				if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				entry, question, _, err := PublishPlanReview(ctx, db, turn, now)
				if err != nil {
					return err
				}
				if name == "empty answer has no review" {
					if entry != nil || question != nil {
						return errors.New("empty answer published a review")
					}
					return nil
				}
				if entry == nil || question == nil {
					return errors.New("hidden note suppressed the review")
				}
				saved, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", turn.String(), question.Entry)
				if err != nil {
					return err
				}
				if saved == nil || planReviewSpec(*saved) == nil || saved.Data.(*canonical.Notice).Data.(*canonical.PlanReview).Source != answer.ID {
					return errors.New("review does not point at the visible answer")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
	for _, kind := range []string{"submit_turn", "queue_turn", "submit_task"} {
		t.Run("new "+kind+" input supersedes completed review", func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModePlan)
			ctx := context.Background()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Plan"))), now)
				record.Provenance.Author = "assistant"
				if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
					return err
				}
				if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
					return err
				}
				_, question, _, err := PublishPlanReview(ctx, db, turn, now)
				if err != nil {
					return err
				}
				if question == nil {
					return errors.New("review was not created")
				}
				message := protocol.ConversationInput{Text: "New instructions"}
				command := protocol.Command{Kind: kind}
				switch kind {
				case "submit_turn":
					command.Data = &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: message}
				case "queue_turn":
					command.Data = &protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: message}
				case "submit_task":
					command.Data = &protocol.CommandSubmitTaskPayload{Session: session.ID, Message: message}
				}
				request := protocol.NewRequest(node, command)
				body, err := encode(request)
				if err != nil {
					return err
				}
				if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); err != nil {
					return err
				}
				output, _, err := ExecuteAgentCommand(ctx, db, node, "", node, request, now)
				if err != nil {
					return err
				}
				changes, _, err := ApplyReviewCommand(ctx, db, request, output, now)
				if err != nil {
					return err
				}
				saved, err := ReadQuestion(ctx, db, question.ID)
				if err != nil {
					return err
				}
				if saved.State.Kind != "cancelled" || len(changes) != 2 {
					return errors.New("new input did not supersede review canonically")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}
