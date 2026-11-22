package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRestoredUnfinishedTurnExpandsOnlyProcess(t *testing.T) {
	const (
		group       = "Read reference"
		failed      = "Inspect source"
		failure     = "Source inspection failed"
		command     = "Fixture command"
		commandFail = "Fixture command exited with code 2"
		liveLabel   = "Next command"
		liveFailure = "New command exited with code 3"
	)
	session, turn, worktree := protocol.SessionID{1}, protocol.TurnID{1}, protocol.WorktreeID{1}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 8
	page := &snapshot.Page
	page.Session, page.Revision = session, 1
	run := protocol.ConversationRun{Session: session, Turn: turn, Worktree: worktree, Kind: "task", Sequence: 1, Revision: 1}
	tool := func(id, name, label, failure string, grouping protocol.ToolGrouping) (protocol.ConversationPart, protocol.ConversationPart) {
		request := transcriptTool(id, name)
		call := request.Data.(protocol.ConversationPartToolCallPayload)
		call.Summary.Title, call.Grouping = &label, grouping
		request.Data = call
		result := transcriptResult(id, name)
		summary := result.Data.(protocol.ConversationPartToolResultPayload).Summary
		summary.Title = &label
		if failure != "" {
			summary.Error, summary.HasDetails = &failure, true
			summary.Failure = &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
		}
		return request, result
	}
	inspect, inspectResult := tool("inspect", "read_file", failed, failure, protocol.ToolGroupingSequence)
	read, readResult := tool("read", "read_file", group, "", protocol.ToolGroupingSequence)
	cmd, cmdResult := tool("command", "run_command", command, commandFail, protocol.ToolGroupingStandalone)
	page.Entries = []protocol.ConversationEntry{
		{ID: "request", Turn: turn, Sequence: 1, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Inspect the workspace"}}},
		{ID: "tools", Turn: turn, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{inspect, read, cmd}},
		{ID: "results", Turn: turn, Sequence: 3, Author: "tool", Parts: []protocol.ConversationPart{inspectResult, readResult, cmdResult}},
		{ID: "delegations", Turn: turn, Sequence: 4, Author: "assistant"},
	}
	for i, title := range []string{"First worker", "Second worker"} {
		part := transcriptTool(title, "spawn_agent")
		call := part.Data.(protocol.ConversationPartToolCallPayload)
		call.Presentation = protocol.ToolPresentationHidden
		call.Summary.Title, call.Summary.Delegation.Background = &title, true
		part.Data = call
		page.Entries[3].Parts = append(page.Entries[3].Parts, part)
		page.Children = append(page.Children, protocol.ConversationChild{
			Origin: protocol.Delegation{Session: session, Turn: turn, Entry: "delegations", Index: uint64(i)},
			Run: protocol.ConversationRun{Session: protocol.SessionID{byte(i + 2)}, Turn: protocol.TurnID{byte(i + 2)}, Worktree: worktree,
				Sequence: uint64(i + 2), Revision: 1, Kind: "task", Status: protocol.ConversationStatusInterrupted},
		})
	}
	for _, status := range []protocol.ConversationStatus{protocol.ConversationStatusInterrupted, protocol.ConversationStatusRunning} {
		t.Run(string(status), func(t *testing.T) {
			run := run
			run.Status = status
			snapshot.Page.Runs = []protocol.ConversationRun{run}
			r, cv, _, frame := watchCallbackTranscript(t, snapshot)
			tester := transcriptActivityTester(&cv.Turns)
			tester.SetLocale("en")
			show := func() {
				t.Helper()
				cacheTestFrame(t, r, cv, session)
				tester.Frame()
				tester.Frame()
			}
			click := func(label string) {
				t.Helper()
				if err := tester.Click(label); err != nil {
					t.Fatalf("cannot click %q: %v; %q", label, err, tester.Texts())
				}
				tester.Frame()
			}
			visible := func(want bool, labels ...string) {
				t.Helper()
				for _, label := range labels {
					if tester.HasText(label) != want {
						t.Fatalf("%q visibility should be %t: %q", label, want, tester.Texts())
					}
				}
			}
			children := i18n.In("en", "subagent_count", "completed", 2, "total", 2)
			show()
			visible(true, group, command, children)
			visible(false, failed, failure, commandFail, "First worker", "Second worker")

			click(group)
			visible(true, failed)
			visible(false, failure)
			click(failed)
			visible(true, failure)
			click(command)
			visible(true, commandFail)
			click(children)
			visible(true, "First worker", "Second worker")

			if status == protocol.ConversationStatusInterrupted {
				process := i18n.In("en", conversation.StatusKey(conversation.Interrupted))
				click(process)
				visible(false, group, command, children)
				click(process)
				visible(true, failure, commandFail, "First worker", "Second worker")
				return
			}

			click(failed)
			request, result := tool("live", "run_command", liveLabel, liveFailure, protocol.ToolGroupingStandalone)
			frame("entry", protocol.ConversationEntry{ID: "live-call", Turn: turn, Sequence: 5, Author: "assistant", Parts: []protocol.ConversationPart{request}})
			frame("entry", protocol.ConversationEntry{ID: "live-result", Turn: turn, Sequence: 6, Author: "tool", Parts: []protocol.ConversationPart{result}})
			show()
			visible(true, liveLabel, failed, commandFail, "First worker", "Second worker")
			visible(false, liveFailure, failure)

			run.Status = protocol.ConversationStatusCancelled
			frame("run", run)
			show()
			visible(true, liveFailure, failed, commandFail, "First worker", "Second worker")
			visible(false, failure)
		})
	}
}
