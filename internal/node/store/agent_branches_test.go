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
)

func TestForkRewindReplace(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"fork shares records", "rewind preserves title and records", "replace is atomic", "replace rolls back admission failure"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			profile, node, storage := schemaFixture(t)
			ctx := context.Background()
			now := time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC)
			var session protocol.Session
			var initialHistoryRevision uint64
			turns := []protocol.TurnID{}
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				providerID := protocol.NewProviderID()
				var provider protocol.ConversationProvider
				body := fmt.Sprintf(`{"id":%q,"revision":1,"name":"Fixture","api":"chat_completions","authentication":"api_key","endpoint":"http://fixture.invalid/v1","enabled":true,"models":[{"id":"fixture","context":32000,"output":8000}],"default_model":"fixture","credential":null}`, providerID.String())
				if err := json.Unmarshal([]byte(body), &provider); err != nil {
					return err
				}
				saved := providerConfiguration{ID: providerID, Provider: &provider, Listed: true, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
				bytes, err := encode(saved)
				if err != nil {
					return err
				}
				if _, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", providerID.String(), bytes); err != nil {
					return err
				}
				config := protocol.DefaultSessionConfig()
				config.Provider = providerID
				config.Model = "fixture"
				config.Mode = protocol.WorkModeCode
				config.Permission = protocol.PermissionAsk
				output, _, err := createSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Config: &config})
				if err != nil {
					return err
				}
				session = output.Data.(protocol.Session)
				initialHistoryRevision, err = historyRevision(ctx, db, session.ID)
				if err != nil {
					return err
				}
				for i := 0; i < 3; i++ {
					input := protocol.DefaultConversationInput()
					input.Text = fmt.Sprintf("Input %d", i)
					request := protocol.NewRequest(node, protocol.Command{Kind: "submit_turn", Data: &protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}})
					bytes, err := encode(request)
					if err != nil {
						return err
					}
					if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), bytes); err != nil {
						return err
					}
					capture, err := admitAgentTurn(ctx, db, node, node, request, session, protocol.ConversationRunKindTask, true)
					if err != nil {
						return err
					}
					inv, _, err := ClaimNext(ctx, db, now.Add(time.Duration(i)*time.Second))
					if err != nil {
						return err
					}
					if inv == nil || inv.Turn.ID != capture.ID {
						return errors.New("claim did not preserve admitted turn")
					}
					user := canonical.NewRecord(capture.ID, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, input.Text))), now)
					user.Provenance.Author = "user"
					if _, _, err = AppendCanonical(ctx, db, session.ID, user); err != nil {
						return err
					}
					reply := canonical.NewRecord(capture.ID, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, fmt.Sprintf("Reply %d", i)))), now)
					reply.Provenance.Author = "sailry"
					if _, _, err = AppendCanonical(ctx, db, session.ID, reply); err != nil {
						return err
					}
					if _, _, err = FinishTurn(ctx, db, capture.ID, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					turns = append(turns, capture.ID)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			before := schemaInteger(t, storage.Node, "SELECT count(*) FROM agent_events")
			switch scenario {
			case "fork shares records":
				err = storage.Node.Write(ctx, func(db *sql.Conn) error {
					fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: turns[1], ExpectedRevision: session.Revision})
					if err != nil {
						return err
					}
					history, err := ReadConversation(ctx, db, fork.ID, nil, 20)
					if err != nil {
						return err
					}
					if len(history.Page.Runs) != 2 || len(history.Page.Entries) != 4 || history.Page.Runs[0].Origin == nil || *history.Page.Runs[0].Origin != session.ID {
						return errors.New("fork lost inherited membership or origin")
					}
					return nil
				})
			case "rewind preserves title and records":
				err = storage.Node.Write(ctx, func(db *sql.Conn) error {
					if _, _, err := renameSession(ctx, db, protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: session.Revision, Title: "Current title"}); err != nil {
						return err
					}
					history, _, err := rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: session.ID, Through: &turns[0], ExpectedHead: turns[2], ExpectedRevision: initialHistoryRevision}, false)
					if err != nil {
						return err
					}
					if history.Revision != initialHistoryRevision+1 {
						return errors.New("history revision did not advance")
					}
					current, err := ReadSession(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if current.Activity.Title != "Current title" {
						return errors.New("rewind changed current title")
					}
					page, err := ReadConversation(ctx, db, session.ID, nil, 20)
					if err != nil {
						return err
					}
					if len(page.Page.Runs) != 1 {
						return errors.New("rewind left later membership")
					}
					return nil
				})
			case "replace rolls back admission failure":
				sessions := schemaInteger(t, storage.Node, "SELECT count(*) FROM sessions")
				if e := storage.Node.Write(ctx, func(db *sql.Conn) error {
					_, e := db.ExecContext(ctx, "CREATE TRIGGER fixture_reject_turn BEFORE INSERT ON turns BEGIN SELECT RAISE(ABORT,'fixture admission failure'); END")
					return e
				}); e != nil {
					t.Fatal(e)
				}
				failure := storage.Node.Write(ctx, func(db *sql.Conn) error {
					message := protocol.DefaultConversationInput()
					message.Text = "Replacement"
					request := protocol.NewRequest(node, protocol.Command{Kind: "replace_turn", Data: &protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: turns[1], ExpectedHead: turns[2], ExpectedRevision: session.Revision, ExpectedHistoryRevision: initialHistoryRevision, Message: message}})
					body, e := encode(request)
					if e != nil {
						return e
					}
					if _, e = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), body); e != nil {
						return e
					}
					_, _, e = replaceConversationTurn(ctx, db, node, node, request)
					return e
				})
				if failure == nil || !strings.Contains(failure.Error(), "fixture admission failure") {
					t.Fatalf("expected injected admission failure, got %v", failure)
				}
				if after := schemaInteger(t, storage.Node, "SELECT count(*) FROM sessions"); after != sessions {
					t.Fatal("failed replacement retained backup")
				}
				err = storage.Node.Read(ctx, func(db *sql.Conn) error {
					revision, e := historyRevision(ctx, db, session.ID)
					if e != nil {
						return e
					}
					if revision != initialHistoryRevision {
						return errors.New("failed replacement changed history revision")
					}
					head, e := conversationHead(ctx, db, session.ID)
					if e != nil {
						return e
					}
					if head == nil || *head != turns[2] {
						return errors.New("failed replacement changed head")
					}
					history, e := ReadConversation(ctx, db, session.ID, nil, 20)
					if e != nil {
						return e
					}
					if len(history.Page.Runs) != 3 || len(history.Page.Entries) != 6 {
						return errors.New("failed replacement changed membership")
					}
					return nil
				})
			case "replace is atomic":
				err = storage.Node.Write(ctx, func(db *sql.Conn) error {
					message := protocol.DefaultConversationInput()
					message.Text = "Replacement"
					request := protocol.NewRequest(node, protocol.Command{Kind: "replace_turn", Data: &protocol.CommandReplaceTurnPayload{Session: session.ID, Turn: turns[1], ExpectedHead: turns[2], ExpectedRevision: session.Revision, ExpectedHistoryRevision: initialHistoryRevision, Message: message}})
					bytes, err := encode(request)
					if err != nil {
						return err
					}
					if _, err = db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')", node[:], request.ID.String(), bytes); err != nil {
						return err
					}
					output, _, err := replaceConversationTurn(ctx, db, node, node, request)
					if err != nil {
						return err
					}
					replaced := output.Data.(protocol.OutputTurnReplacedPayload)
					if replaced.History.Backup == nil || replaced.History.Revision != initialHistoryRevision+1 {
						return errors.New("replacement did not preserve backup/revision")
					}
					history, err := ReadConversation(ctx, db, session.ID, nil, 20)
					if err != nil {
						return err
					}
					if len(history.Page.Runs) != 2 || history.Page.Runs[1].Turn != replaced.Turn.ID {
						return errors.New("replacement was not admitted with rewind")
					}
					return nil
				})
			}
			if err != nil {
				t.Fatal(err)
			}
			if after := schemaInteger(t, storage.Node, "SELECT count(*) FROM agent_events"); after != before {
				t.Fatalf("branch changed canonical record count: %d -> %d", before, after)
			}
		})
	}
}
func ptr[T any](value T) *T { return &value }
