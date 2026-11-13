package acceptance

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func liveAdmissionHistory(session protocol.SessionID, turn protocol.TurnID, input string) protocol.ConversationHistory {
	history := protocol.DefaultConversationHistory()
	history.Sequence, history.Page.Session, history.Page.Revision = 2, session, 1
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Worktree = session, turn, protocol.WorktreeID{4}
	run.Sequence, run.Revision = 1, 1
	run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusCompleted
	history.Page.Runs = []protocol.ConversationRun{run}
	for index, text := range []string{input, "Fixture answer"} {
		entry := protocol.DefaultConversationEntry()
		entry.ID, entry.Sequence, entry.Turn = []string{"input", "answer"}[index], uint64(index+1), turn
		entry.Author = []string{"user", "assistant"}[index]
		entry.Parts = []protocol.ConversationPart{{Kind: "text", Data: text}}
		history.Page.Entries = append(history.Page.Entries, entry)
	}
	return history
}

func TestLiveDurableRequiresSingleAdmission(t *testing.T) {
	const input = liveScope + "One fixture message"
	for _, fixture := range []string{"single", "duplicate_task", "duplicate_task_identity", "duplicate_user", "duplicate_user_turn", "wrong_input", "wrong_page_session", "wrong_task_session", "wrong_user_turn", "wrong_observed_turn", "missing_user", "paged_history", "missing_history", "queued_admission"} {
		t.Run(fixture, func(t *testing.T) {
			session, turn := protocol.SessionID{2}, protocol.TurnID{3}
			history := liveAdmissionHistory(session, turn, input)
			result := core.CaseResult{}
			cv := &viewmodel.Conversation{ID: session.String(), Turns: []conversation.Turn{{ID: turn.String(), User: input,
				Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: "Fixture answer"}}}}}}
			w := &liveWindow{window: &window{ctx: t.Context(), result: &result, app: shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})},
				session: protocol.Session{ID: session}, turn: turn, input: input}
			if got, admitted, err := w.admittedTurn(history.Page); err != nil || !admitted || got != turn {
				t.Fatalf("the original observation did not admit the fixture once: %v", err)
			}
			// The first task and its answer stay valid. The final history must
			// still fail if a second admission appeared after that observation.
			switch fixture {
			case "duplicate_task", "duplicate_task_identity":
				extra := history.Page.Runs[0]
				extra.Sequence = 2
				if fixture == "duplicate_task" {
					extra.Turn = protocol.TurnID{8}
				}
				history.Page.Runs = append(history.Page.Runs, extra)
			case "duplicate_user", "duplicate_user_turn":
				extra := history.Page.Entries[0]
				extra.ID, extra.Sequence = "extra-input", 3
				if fixture == "duplicate_user_turn" {
					extra.Turn = protocol.TurnID{8}
				}
				history.Page.Entries = append(history.Page.Entries, extra)
			case "wrong_input":
				history.Page.Entries[0].Parts = []protocol.ConversationPart{{Kind: "text", Data: "Changed fixture input"}}
			case "wrong_page_session":
				history.Page.Session = protocol.SessionID{8}
			case "wrong_task_session":
				history.Page.Runs[0].Session = protocol.SessionID{8}
			case "wrong_user_turn":
				history.Page.Entries[0].Turn = protocol.TurnID{8}
			case "wrong_observed_turn":
				w.turn = protocol.TurnID{8}
			case "missing_user":
				history.Page.Entries = history.Page.Entries[1:]
			case "paged_history":
				history.Page.NextBefore = &turn
			case "missing_history":
				history.Missing = []protocol.TurnID{turn}
			case "queued_admission":
				pending := protocol.DefaultConversationPending()
				pending.Turn, pending.Kind, pending.Revision, pending.ConfigRevision = protocol.TurnID{8}, protocol.ConversationRunKindTask, 1, 1
				history.Page.Queue.Items = []protocol.ConversationPending{pending}
			}
			reads := 0
			w.controller = client.New(&liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				if request.Command.Kind != "read_conversation" {
					t.Fatalf("admission assertion attempted %q instead of a read", request.Command.Kind)
				}
				query := request.Command.Data.(*protocol.CommandReadConversationPayload)
				if query.Session != session {
					t.Fatal("admission assertion read another session")
				}
				reads++
				return protocol.Output{Kind: "conversation", Data: history}, nil
			}})
			t.Cleanup(func() { _ = w.controller.Close() })
			err := w.assertDurable(protocol.ConversationStatusCompleted)
			if fixture == "single" {
				if err != nil || len(result.Evidence) != 1 || !strings.Contains(result.Evidence[0].Value, "Exactly one task and user message") {
					t.Fatalf("one unchanged admission was not confirmed: %v, %+v", err, result.Evidence)
				}
			} else {
				var issue *liveIssue
				if !errors.As(err, &issue) || issue.status != "failed" || len(result.Evidence) != 0 {
					t.Fatalf("%s passed or lost its assertion failure: %v", fixture, err)
				}
			}
			if reads != 1 {
				t.Fatalf("admission assertion performed %d reads", reads)
			}
		})
	}
}

type liveAdmissionActions struct {
	shell.ConversationActions
	send func(composer.Message)
}

func (actions *liveAdmissionActions) Send(message composer.Message) { actions.send(message) }

func TestLiveSendButtonAndEnterAdmitTheExactInputOnce(t *testing.T) {
	for _, method := range []struct {
		name string
		send func(*liveWindow, string) error
	}{
		{"button", (*liveWindow).send},
		{"enter", (*liveWindow).sendWithEnter},
	} {
		t.Run(method.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			session, turn := protocol.SessionID{2}, protocol.TurnID{3}
			transport := &recordedLiveTransport{updates: make(chan protocol.Update, 2), liveFixtureTransport: &liveFixtureTransport{
				dispatch: func(context.Context, protocol.Request) (protocol.Output, error) {
					t.Error("the input fixture must not execute a Node command")
					return protocol.Output{}, errors.New("input fixture has no commands")
				},
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			w := &liveWindow{window: &window{ctx: ctx, result: &core.CaseResult{}}, controller: controller, session: protocol.Session{ID: session},
				wake: make(chan struct{}, 1), lastFrame: time.Now()}
			release, err := controller.WatchConversation(ctx, session, w.notify)
			if err != nil {
				t.Fatal(err)
			}
			defer release()
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Node, snapshot.Page.Session = protocol.NodeID{1}, session
			snapshot.Page.Revision = 1
			transport.updates <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
			for {
				if state, known := controller.Conversation(session); known {
					if state.Err != nil {
						t.Fatalf("initial conversation fixture was rejected: %v", state.Err)
					}
					if state.Connected {
						break
					}
				}
				select {
				case <-w.wake:
				case <-ctx.Done():
					t.Fatal(ctx.Err())
				}
			}
			cv := &viewmodel.Conversation{ID: session.String(), Title: "Input fixture", Session: conversation.Session{Connection: conversation.Connected}}
			models := []composer.Model{{ID: "fixture-model", Name: "Fixture model"}}
			w.app = shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Models: models, Workspace: &viewmodel.Workspace{}})
			w.app.Effects, w.app.NavOpen, w.app.Sidebar.Selected = false, false, cv.ID
			admissions := 0
			actions := &liveAdmissionActions{send: func(message composer.Message) {
				admissions++
				history := liveAdmissionHistory(session, turn, message.Text)
				snapshot.Sequence, snapshot.Page = history.Sequence, history.Page
				transport.updates <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
			}}
			w.app.Hooks.Conversation = func(*ui.Context, *viewmodel.Conversation, *composer.State) (*composer.Context, shell.ConversationActions) {
				return &composer.Context{Models: models, Activity: composer.Activity{Connected: true}}, actions
			}
			w.tester = ui.NewTesterWith(w.app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
			w.tester.SetLocale("en")
			const text = "Send this one fixture message"
			if err := method.send(w, text); err != nil {
				t.Fatal(err)
			}
			if admissions != 1 || w.turn != turn || w.input != liveScope+text || w.app.Composer(cv.ID).Draft != "" {
				t.Fatalf("%s did not admit and clear exactly the typed input: admissions=%d, turn=%v", method.name, admissions, w.turn)
			}
		})
	}
}
