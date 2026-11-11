package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestQueueStates covers what the queue tray and the transcript read from the
// Node's queue: plain rows whatever they wait on, the halt that is reported
// once, no rows for held background results, what became of guidance sent
// from the composer, which turn pending guidance belongs to, and how an
// approval closed by Stop reads.
func TestQueueStates(t *testing.T) {
	session := protocol.SessionID{1}
	running, ended, item := protocol.TurnID{1}, protocol.TurnID{2}, protocol.TurnID{3}
	base := func() protocol.ConversationPage {
		page := protocol.DefaultConversationPage()
		page.Session = session
		page.Runs = []protocol.ConversationRun{
			{Session: session, Turn: ended, Kind: "task", Status: "cancelled"},
			{Session: session, Turn: running, Kind: "task", Status: "running"},
		}
		return page
	}
	// The tray lists queued input as plain rows: the Node's pause reason
	// decides only whether a halt is reported, once, as a toast.
	t.Run("halt", func(t *testing.T) {
		for _, row := range []struct {
			name   string
			change func(*protocol.ConversationPage, *protocol.ConversationPending)
			want   protocol.ConversationQueuePause
		}{
			{"queued in order", func(*protocol.ConversationPage, *protocol.ConversationPending) {}, ""},
			{"held", func(_ *protocol.ConversationPage, pending *protocol.ConversationPending) { pending.Ready = false }, ""},
			{"paused by the user", func(page *protocol.ConversationPage, _ *protocol.ConversationPending) {
				page.Queue.Paused, page.Queue.PauseReason = true, new(protocol.ConversationQueuePauseUser)
			}, ""},
			{"paused by stop", func(page *protocol.ConversationPage, _ *protocol.ConversationPending) {
				page.Queue.Paused, page.Queue.PauseReason = true, new(protocol.ConversationQueuePauseStopped)
			}, ""},
			{"paused without a reason", func(page *protocol.ConversationPage, _ *protocol.ConversationPending) {
				// An older Node, or a failed child session.
				page.Queue.Paused = true
			}, ""},
			{"a turn lost its owner", func(page *protocol.ConversationPage, _ *protocol.ConversationPending) {
				page.Queue.Paused, page.Queue.PauseReason = true, new(protocol.ConversationQueuePauseInterrupted)
			}, protocol.ConversationQueuePauseInterrupted},
			{"input was not recorded", func(page *protocol.ConversationPage, _ *protocol.ConversationPending) {
				page.Queue.Paused, page.Queue.PauseReason = true, new(protocol.ConversationQueuePauseUnrecorded)
				page.Runs = append(page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "failed"})
			}, protocol.ConversationQueuePauseUnrecorded},
			{"resumed with a stale reason", func(page *protocol.ConversationPage, _ *protocol.ConversationPending) {
				page.Queue.PauseReason = new(protocol.ConversationQueuePauseInterrupted)
			}, ""},
			{"only a notice waits", func(page *protocol.ConversationPage, pending *protocol.ConversationPending) {
				page.Queue.Paused, page.Queue.PauseReason = true, new(protocol.ConversationQueuePauseInterrupted)
				pending.Notice = true
			}, ""},
		} {
			t.Run(row.name, func(t *testing.T) {
				page := base()
				pending := protocol.ConversationPending{Turn: item, Kind: "task", Ready: true, Preview: "Next"}
				row.change(&page, &pending)
				page.Queue.Items = []protocol.ConversationPending{pending}
				if got := queueHalt(page); got != row.want {
					t.Fatalf("halt = %q, want %q", got, row.want)
				}
				// Whatever it waits on, the row is the same plain row.
				_, _, _, queue := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
				want := composer.Queued{ID: item.String(), Text: "Next", Notice: pending.Notice}
				if len(queue) != 1 || queue[0].ID != want.ID || queue[0].Text != want.Text || queue[0].Notice != want.Notice || queue[0].Guided || queue[0].Joining {
					t.Fatalf("projected queue = %+v, want %+v", queue, want)
				}
			})
		}
		page := base()
		page.Queue.Paused, page.Queue.PauseReason = true, new(protocol.ConversationQueuePauseInterrupted)
		if got := queueHalt(page); got != "" {
			t.Fatalf("an empty queue halts on %q", got)
		}
	})
	t.Run("a halt is reported when it begins, once", func(t *testing.T) {
		const interrupted, unsent = protocol.ConversationQueuePauseInterrupted, protocol.ConversationQueuePauseUnrecorded
		r := &Runtime{}
		for step, row := range []struct {
			id   string
			halt protocol.ConversationQueuePause
			want bool
		}{
			{"old", interrupted, false}, // halted before this desktop read it: a restart repeats nothing
			{"old", interrupted, false},
			{"new", "", false},
			{"new", unsent, true},
			{"new", unsent, false}, // every later frame
			{"new", "", false},     // sent again
			{"new", unsent, true},  // and failed again
			{"new", interrupted, true},
			{"old", "", false},
			{"old", interrupted, true},
		} {
			if got := r.haltBegan(row.id, row.halt); got != row.want {
				t.Fatalf("step %d: %s reads %q: reported %v, want %v", step, row.id, row.halt, got, row.want)
			}
		}
	})
	// Background results held back from delivery are not queue rows
	// (docs/removed-ui.md): the tray has nothing to draw for them.
	t.Run("held results are not rows", func(t *testing.T) {
		page := base()
		for _, hold := range []protocol.ConversationBackgroundHold{protocol.ConversationBackgroundHoldLimit, protocol.ConversationBackgroundHoldStopped,
			protocol.ConversationBackgroundHoldInterrupted, protocol.ConversationBackgroundHoldFailed, protocol.ConversationBackgroundHoldOther} {
			task := protocol.DefaultConversationBackgroundTask()
			task.ID, task.Session, task.Title, task.State = string(hold), session, "Build "+string(hold), protocol.ConversationBackgroundStatePending
			task.Hold, task.Suppressed = &hold, true
			page.Background = append(page.Background, task)
		}
		_, _, _, queue := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
		if len(queue) != 0 {
			t.Fatalf("held results became queue rows: %+v", queue)
		}
	})
	t.Run("guidance fate", func(t *testing.T) {
		for _, row := range []struct {
			name   string
			sent   sentGuidance
			change func(*protocol.ConversationPage)
			want   guidanceFate
		}{
			{"not listed yet", sentGuidance{recipient: running}, func(*protocol.ConversationPage) {}, guidanceWaiting},
			{"waiting for its recipient", sentGuidance{recipient: running}, func(page *protocol.ConversationPage) {
				page.Queue.Items = []protocol.ConversationPending{{Turn: item, Ready: true, Guidance: &running}}
			}, guidanceGuiding},
			{"other settings", sentGuidance{recipient: running}, func(page *protocol.ConversationPage) {
				page.Queue.Items = []protocol.ConversationPending{{Turn: item, Ready: true}}
			}, guidanceSeparated},
			{"edited by the user", sentGuidance{recipient: running, guiding: true}, func(page *protocol.ConversationPage) {
				page.Queue.Items = []protocol.ConversationPending{{Turn: item, Ready: true}}
			}, guidanceSettled},
			{"withdrawn", sentGuidance{recipient: running, guiding: true}, func(page *protocol.ConversationPage) {
				page.Queue.Items = []protocol.ConversationPending{{Turn: item}}
			}, guidanceSettled},
			{"recipient ended first", sentGuidance{recipient: ended, guiding: true}, func(page *protocol.ConversationPage) {
				page.Queue.Items = []protocol.ConversationPending{{Turn: item, Ready: true}}
			}, guidanceOutlived},
			{"recipient ended before it was listed", sentGuidance{recipient: ended}, func(page *protocol.ConversationPage) {
				page.Queue.Items = []protocol.ConversationPending{{Turn: item, Ready: true}}
			}, guidanceOutlived},
			{"joined", sentGuidance{recipient: running, guiding: true}, func(page *protocol.ConversationPage) {
				page.Runs = append(page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "completed", Guidance: &running, Joined: &running})
			}, guidanceSettled},
			{"joined before it was listed", sentGuidance{recipient: running}, func(page *protocol.ConversationPage) {
				page.Runs = append(page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "completed", Guidance: &running, Joined: &running})
			}, guidanceSettled},
			{"admitted, queue not read yet", sentGuidance{recipient: running}, func(page *protocol.ConversationPage) {
				page.Runs = append(page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "queued", Guidance: &running})
			}, guidanceWaiting},
		} {
			t.Run(row.name, func(t *testing.T) {
				page := base()
				row.change(&page)
				if got := guidanceOutcome(page, item, row.sent); got != row.want {
					t.Fatalf("fate = %d, want %d", got, row.want)
				}
			})
		}
		queue := protocol.ConversationQueue{Items: []protocol.ConversationPending{{Turn: item, Ready: true}, {Turn: ended, Ready: true, Guidance: &running}}}
		if !guidanceSeparate(queue, item) || guidanceSeparate(queue, ended) || guidanceSeparate(queue, running) {
			t.Fatal("guide_queued_turn answer misread")
		}
	})
	t.Run("pending guidance belongs to the turn that takes it", func(t *testing.T) {
		later := protocol.TurnID{9}
		for _, row := range []struct {
			name   string
			change func(*protocol.ConversationSnapshot)
			wait   int
		}{
			{"none", func(*protocol.ConversationSnapshot) {}, 1},
			{"queued for this turn", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "queued", Guidance: &running})
			}, 0},
			{"joined, not recorded yet", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "completed", Guidance: &running, Joined: &running})
			}, 0},
			{"outlived batch joined this successor", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "completed", Guidance: &ended, Joined: &running})
			}, 0},
			{"sent to this turn, joined a later one", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "completed", Guidance: &running, Joined: &later})
			}, 1},
			{"joined and recorded", func(snapshot *protocol.ConversationSnapshot) {
				snapshot.Page.Runs = append(snapshot.Page.Runs, protocol.ConversationRun{Session: session, Turn: item, Kind: "task", Status: "completed", Guidance: &ended, Joined: &running})
				snapshot.Page.Entries = append(snapshot.Page.Entries, protocol.ConversationEntry{ID: "guidance", Turn: item, Author: "user"})
			}, 1},
		} {
			t.Run(row.name, func(t *testing.T) {
				callID, name := "delegation", "Research"
				page := base()
				page.Entries = []protocol.ConversationEntry{
					{ID: "spawn", Turn: running, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &callID, Name: "spawn_agent", Summary: &protocol.ToolDetailSummary{}}}}},
					{ID: "result", Turn: running, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &callID, Name: "spawn_agent"}}}},
				}
				page.Children = []protocol.ConversationChild{{Run: protocol.ConversationRun{Session: protocol.SessionID{2}, Turn: protocol.TurnID{4}, Kind: "task", Status: "running", Sequence: 2}, Origin: protocol.Delegation{Session: session, Turn: running, Entry: "spawn"}, Name: &name}}
				snapshot := protocol.ConversationSnapshot{Page: page}
				row.change(&snapshot)
				if got := waitingConversationChildren(snapshot, running); got != row.wait {
					t.Fatalf("waiting children = %d, want %d", got, row.wait)
				}
			})
		}
	})
	t.Run("approval closed by stop is a plain row", func(t *testing.T) {
		for _, row := range []struct {
			state   protocol.ConversationApprovalState
			want    conversation.RowState
			warning bool
		}{
			{protocol.ConversationApprovalStateInterrupted, conversation.StateInterrupted, true},
			{protocol.ConversationApprovalStateCancelled, conversation.StateNone, false},
		} {
			t.Run(string(row.state), func(t *testing.T) {
				page := protocol.DefaultConversationPage()
				page.Session = session
				page.Runs = []protocol.ConversationRun{{Session: session, Turn: ended, Kind: "task", Status: "cancelled"}}
				page.Entries = []protocol.ConversationEntry{{ID: "call", Turn: ended, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{Name: "apply_changes", Summary: &protocol.ToolDetailSummary{}}}}}}
				page.Approvals = []protocol.ConversationApproval{{ID: protocol.NewApprovalID(), Session: session, Turn: ended, Entry: "call", State: row.state}}
				turns, _, waiting, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
				if len(waiting.Approvals) != 0 {
					t.Fatalf("closed approval still asks: %+v", waiting.Approvals)
				}
				var calls []conversation.Call
				for _, turn := range turns {
					if turn.Reply.Status != conversation.Cancelled {
						t.Fatalf("stopped turn status = %v", turn.Reply.Status)
					}
					for _, block := range turn.Reply.Blocks {
						calls = append(calls, block.Calls...)
					}
				}
				if len(calls) != 1 || calls[0].State != row.want || calls[0].Warning != row.warning || calls[0].Pending || calls[0].Running || calls[0].Failed {
					t.Fatalf("calls = %+v, want state %d", calls, row.want)
				}
			})
		}
	})
}
