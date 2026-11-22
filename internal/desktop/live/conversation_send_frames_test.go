package live

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMountedSendAdmissionFrames(t *testing.T) {
	const message = "Check the remaining files"
	const earlier = "Inspect the project"
	const locale = "en"
	for _, tc := range []struct {
		name, command           string
		history, busy, ackFirst bool
	}{
		{"empty session", "submit_turn", false, false, false},
		{"idle with history", "submit_turn", true, false, true},
		{"guide during child call", "guide_turn", true, true, true},
		{"queue during child call", "queue_turn", true, true, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			id, owner, sent := protocol.SessionID{1}, protocol.TurnID{1}, protocol.TurnID{2}
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 10
			snapshot.Page.Session, snapshot.Page.Revision = id, 1
			if tc.history {
				snapshot.Page = cacheTestPage(id, 1, earlier)
			}
			if tc.busy {
				parent := &snapshot.Page.Runs[0]
				parent.Status, parent.FinishedMs = protocol.ConversationStatusRunning, nil
				call := &snapshot.Page.Entries[1]
				call.Parts = []protocol.ConversationPart{transcriptTool("child", "spawn_agent")}
				snapshot.Page.Children = []protocol.ConversationChild{{
					Origin: protocol.Delegation{Session: id, Turn: owner, Entry: call.ID},
					Run:    protocol.ConversationRun{Session: protocol.SessionID{3}, Turn: protocol.TurnID{3}, Kind: "task", Status: "running", Revision: 1, Sequence: 2},
				}}
			}
			r, cv, transport, publish := watchCallbackTranscript(t, snapshot)
			model := protocol.DefaultConversationModel()
			model.ID = "fixture"
			session, _ := r.session(cv.ID)
			session.Config.Provider, session.Config.Model = protocol.ProviderID{1}, model.ID
			session.Config.Mode, session.Config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			r.snapshotCopy, r.snapshotRead = r.snapshot(), true
			r.snapshotCopy.Sessions = []protocol.Session{session}
			r.snapshotCopy.Providers = []protocol.ConversationProvider{{ID: session.Config.Provider, Enabled: true, Models: []protocol.ConversationModel{model}}}
			comp := r.App.Composer(cv.ID)
			r.composerBases = make(map[*composer.State]protocol.SessionConfig)
			r.applyComposer(comp, session.Config)
			models := []composer.Model{{ID: comp.Model, Name: model.ID}}
			list := r.App.Messages.List(cv.ID)
			requests := make(chan protocol.Request, 1)
			transport.handle = func(req protocol.Request) (link.Response, bool) {
				switch req.Command.Kind {
				case "count_feature":
					return link.Response{Output: protocol.Output{Kind: "accepted"}}, true
				case "submit_turn", "guide_turn", "queue_turn":
					requests <- req
					return link.Response{Output: protocol.Output{Kind: "queued_turn", Data: protocol.QueuedTurn{ID: sent, Session: id, Request: req.ID}}}, true
				}
				return link.Response{}, false
			}
			var ctx *composer.Context
			var ack bool
			tt := ui.NewTesterWith(func(c *ui.Context) {
				if c.Locale() != locale {
					return
				}
				theme.Apply(c, "", theme.Light)
				if ack {
					applyOverviewCallbacks(r, c)
				}
				r.beginConversationFrame()
				defer r.finishConversationFrame()
				r.conversationSeen[id] = true
				state, _ := r.conversationState(id)
				r.projectWatchedConversation(c, cv, id, state)
				comp.Running = cv.Running()
				x := &conversationActions{r: r, c: c, cv: cv, comp: comp}
				ctx = &composer.Context{Models: models, Queue: cv.Queue, Activity: composer.Activity{Connected: true}}
				r.projectComposerQueue(x, ctx)
				ui.Column(c).Fill().Children(func() {
					conversation.Docked(c, r.App.Messages, cv.ID, cv.Turns, &cv.Session, x, func() { composer.View(c, comp, ctx, x) })
				})
			}, 1050, 950, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
			tt.SetLocale(locale)
			tt.SetFocused(true)
			tt.Move(-1, -1)
			comp.Focus()
			tt.Frame()
			tt.Advance(time.Second / 60)
			editor := i18n.In(locale, "composer")
			field, ok := tt.AccessByRole(ui.RoleTextField, editor)
			if !ok || !field.Focused || field.Bounds.W <= 0 || field.Bounds.H <= 0 {
				t.Fatalf("draft editor hidden or unfocused before typing: %+v", field)
			}
			tt.Type(message)
			tt.Frame()
			if comp.Draft != message || comp.Running != tc.busy {
				t.Fatalf("draft %q, running %t", comp.Draft, comp.Running)
			}
			if tc.busy {
				blocks := cv.Turns[0].Reply.Blocks
				if len(blocks) != 1 || len(blocks[0].Children) != 1 || blocks[0].Children[0].Status != conversation.ChildRunning {
					t.Fatal("the parent's child call is missing from the mounted transcript")
				}
			}
			if tc.command == "queue_turn" {
				tt.Key(0, ui.KeyTab)
			} else {
				tt.Key(0, ui.KeyEnter)
			}
			tt.Frame()
			r.workers.Wait()
			var req protocol.Request
			select {
			case req = <-requests:
			default:
				t.Fatal("the editor did not submit the draft")
			}
			if req.Command.Kind != tc.command {
				t.Fatalf("editor sent %q, want %q", req.Command.Kind, tc.command)
			}
			if tc.command == "guide_turn" {
				guide, ok := detail[protocol.CommandGuideTurnPayload](req.Command.Data)
				if !ok || guide.Recipient != owner {
					t.Fatalf("guidance sent to %v, want %v", guide.Recipient, owner)
				}
			}

			// Render the first frame of every publication, including the frames
			// with only the run preview or the canonical entry before Joined.
			parentActive := tc.busy
			check := func(stage string, rows int, localTray, admittedTray bool) {
				t.Helper()
				tt.Frame()
				field, ok := tt.AccessByRole(ui.RoleTextField, editor)
				if !ok || !field.Focused || field.Bounds.H <= 0 || r.App.Composer(cv.ID) != comp {
					t.Fatalf("%s: composer hidden, unfocused or remounted: %+v", stage, field)
				}
				if log, ok := tt.AccessByRole(ui.RoleLog, ""); !ok || log.Bounds.H <= 0 || r.App.Messages.List(cv.ID) != list {
					t.Fatalf("%s: transcript hidden or remounted", stage)
				}
				if len(cv.Turns) != rows || (ctx.Outgoing != nil) != localTray || len(ctx.Queue) > 1 || (len(ctx.Queue) == 1) != admittedTray {
					t.Fatalf("%s: %d transcript rows, local tray %v, queue %+v", stage, len(cv.Turns), ctx.Outgoing, ctx.Queue)
				}
				if tc.history && (cv.Turns[0].ID != owner.String() || !tt.HasText(earlier)) {
					t.Fatalf("%s: earlier input displaced: %q", stage, tt.Texts())
				}
				if parentActive && (cv.Turns[0].Reply.Status != conversation.Running || !comp.Running) {
					t.Fatalf("%s: lost the running parent", stage)
				}
				if localTray || admittedTray {
					if ctx.Outgoing != nil && ctx.Outgoing.Text != message || admittedTray && (ctx.Queue[0].ID != sent.String() || ctx.Queue[0].Text != message) || cv.Turns[0].User != earlier {
						t.Fatalf("%s: pending input escaped its tray: %+v", stage, cv.Turns)
					}
					singleQueued(t, tt, message)
				} else if !tt.HasText(message) || (tc.command != "guide_turn" && cv.Turns[rows-1].User != message) {
					t.Fatalf("%s: missing input from transcript: %q", stage, tt.Texts())
				}
			}
			base := len(snapshot.Page.Runs)
			waiting := func(stage string, local bool) {
				if tc.busy {
					check(stage, base, local, !local)
				} else {
					check(stage, base+1, false, false)
					if !cv.Turns[base].InputOnly || comp.Running {
						t.Fatalf("%s: admission started an assistant before claim", stage)
					}
				}
			}
			waiting("local submission", true)
			if tc.ackFirst {
				ack = true
				waiting("response before publication", true)
			}
			run := protocol.ConversationRun{Session: id, Turn: sent, Kind: "task", Status: "queued", Sequence: 3, Revision: 1}
			input := protocol.ConversationSentInput{Message: protocol.ConversationInput{Text: message}}
			item := protocol.ConversationPending{Turn: sent, Request: &req.ID, Kind: "task", Preview: message, Revision: 1, ConfigRevision: 1, Ready: tc.command != "queue_turn"}
			if tc.command == "guide_turn" {
				run.Guidance, run.Input, item.Guidance = &owner, &input, &owner
			} else if !tc.busy {
				item.Preview, item.Truncated = "Check the", true
			}
			publish("run", run)
			waiting("run before queue", true)
			queue := protocol.ConversationQueue{Revision: snapshot.Page.Queue.Revision + 1, Items: []protocol.ConversationPending{item}}
			publish("queue", queue)
			waiting("admitted queue", false)
			ack = true
			waiting("acknowledged queue", false)
			if comp.Draft != "" {
				t.Fatalf("acknowledged draft still in the editor: %q", comp.Draft)
			}
			entry := protocol.ConversationEntry{ID: "sent", Sequence: 3, Turn: sent, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: message}}}
			if tc.command == "guide_turn" {
				publish("entry", entry)
				waiting("entry before joined run", false)
				run.Status, run.Joined, run.Input = protocol.ConversationStatusCompleted, &owner, nil
				run.StartedMs, run.FinishedMs = snapshot.Page.Runs[0].StartedMs, snapshot.Page.Runs[0].StartedMs
				publish("run", run)
				check("joined with stale queue", 1, false, false)
				if !outgoingInputShown(cv, sent.String()) {
					t.Fatal("consumed guidance missing from the parent's input segment")
				}
			} else {
				if tc.busy {
					parentActive = false
					parent := snapshot.Page.Runs[0]
					parent.Status, parent.FinishedMs = protocol.ConversationStatusCompleted, new(int64(2000))
					publish("run", parent)
					waiting("parent finished", false)
					item.Ready = true
					item.Revision++
					queue.Revision++
					queue.Items = []protocol.ConversationPending{item}
					publish("queue", queue)
					waiting("ready to start", false)
				}
				run.Status, run.StartedMs = protocol.ConversationStatusRunning, new(int64(3000))
				publish("run", run)
				check("claimed before input with stale queue", base+1, false, false)
				if cv.Turns[base].ID != sent.String() || cv.Turns[base].Reply.Status != conversation.Running {
					t.Fatalf("claim did not adopt the sent input: %+v", cv.Turns[base])
				}
				publish("entry", entry)
				check("canonical input", base+1, false, false)
			}
			queue.Items, queue.Revision = nil, queue.Revision+1
			publish("queue", queue)
			if tc.command == "guide_turn" {
				check("queue removal", base, false, false)
			} else {
				check("queue removal", base+1, false, false)
			}
		})
	}
}
