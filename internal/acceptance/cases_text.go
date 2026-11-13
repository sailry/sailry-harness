package acceptance

import (
	"encoding/json"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func caseContext(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	view, err := run.ask("Remember this test-only marker for this conversation: sailry-context-4826. Reply with exactly ACK. Do not call any tools.")
	if err != nil {
		return err
	}
	if err := run.requireText(view, "ACK"); err != nil {
		return err
	}
	view, err = run.ask("Return only the test-only marker I gave you in the previous turn. Do not call any tools.")
	if err != nil {
		return err
	}
	if len(view.tools()) != 0 {
		return fail("Plain context recall unexpectedly called a tool")
	}
	return run.requireText(view, "sailry-context-4826")
}

func caseMarkdown(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	view, err := run.ask("Return Markdown containing the heading ## Acceptance, a two-column table, a fenced Go code block containing func Add(a, b int) int { return a + b }, and a link to https://example.com. Do not call tools or fetch the link.")
	if err != nil {
		return err
	}
	for _, expected := range []string{"## Acceptance", "|", "```go", "func Add", "https://example.com"} {
		if err := run.requireText(view, expected); err != nil {
			return err
		}
	}
	run.evidence("scope", "Canonical Markdown only; UI rendering is a separate case")
	return nil
}

func caseMath(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	view, err := run.ask("Return exactly the following mathematical source, without tools: Inline $a^2+b^2=c^2$ followed by a separate display block $$x=\\frac{-b\\pm\\sqrt{b^2-4ac}}{2a}$$.")
	if err != nil {
		return err
	}
	for _, expected := range []string{"a^2+b^2=c^2", "\\frac", "\\sqrt"} {
		if err := run.requireText(view, expected); err != nil {
			return err
		}
	}
	run.evidence("scope", "Canonical mathematical source only; UI rendering is separate")
	return nil
}

func caseUnicode(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	// Intentional Unicode fixture, not product copy.
	const fixture = "中文 日本語 🎉 مرحبا"
	view, err := run.ask("Repeat this exact test fixture without tools or explanations: " + fixture)
	if err != nil {
		return err
	}
	return run.requireText(view, fixture)
}

func caseStop(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	turn, err := run.submit("Without tools, start immediately with SAILRY-STREAM-START and continue with one plain-text paragraph of at least 1600 words about organizing a workspace. This is a longer streaming sample for the stop control. Do not plan, count, use a list, or add a preamble; keep writing the paragraph until it is complete.")
	if err != nil {
		return err
	}
	view, err := run.wait(turn.ID, func(view turnView) (bool, error) {
		return view.run.Status == protocol.ConversationStatusRunning && view.hasDraft(), nil
	})
	if err != nil {
		return err
	}
	if !active(view.run.Status) {
		return fail("Reply finished before an active streaming stop could be exercised")
	}
	if _, err := run.command(run.ctx, "stop_turn", protocol.CommandStopTurnPayload{Turn: turn.ID}); err != nil {
		return err
	}
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCancelled && view.run.Status != protocol.ConversationStatusInterrupted {
		return fail("Stopped reply did not reach a canceled or interrupted terminal state")
	}
	if err := run.assertCapture(turn); err != nil {
		return err
	}
	run.evidence("state", string(view.run.Status))
	view, err = run.ask("Reply only STOP-RECOVERED, without tools.")
	if err != nil {
		return err
	}
	return run.requireText(view, "STOP-RECOVERED")
}

func caseConversationSearch(run *caseRun) error {
	if err := run.standard(); err != nil {
		return err
	}
	if _, err := run.ask("Without tools, reply exactly SEARCH-SAILRY-6021."); err != nil {
		return err
	}
	output, err := run.command(run.ctx, "search_conversation", protocol.CommandSearchConversationPayload{Session: run.session.ID, Query: protocol.ConversationSearchQuery{Text: "SEARCH-SAILRY-6021", Limit: 20}})
	if err != nil {
		return err
	}
	encoded, err := json.Marshal(output.Data)
	if err != nil || !strings.Contains(string(encoded), "SEARCH-SAILRY-6021") {
		return fail("Conversation search did not return the canonical fixture marker")
	}
	run.evidence("search", "Canonical text match returned")
	return nil
}

func (run *caseRun) branchFixture() ([]protocol.QueuedTurn, error) {
	if err := run.standard(); err != nil {
		return nil, err
	}
	for _, marker := range []string{"BRANCH-ONE", "BRANCH-TWO", "BRANCH-THREE"} {
		if _, err := run.ask("Without tools, reply exactly " + marker + "."); err != nil {
			return nil, err
		}
	}
	return slices.Clone(run.turns), nil
}

func caseRetry(run *caseRun) error { return replaceReply(run, false) }

func caseEdit(run *caseRun) error { return replaceReply(run, true) }

func replaceReply(run *caseRun, edited bool) error {
	if err := run.standard(); err != nil {
		return err
	}
	const original = "Without tools, reply exactly ORIGINAL-REPLY."
	first, err := run.ask(original)
	if err != nil {
		return err
	}
	if err := run.refreshSession(); err != nil {
		return err
	}
	history, err := run.controller.ReadConversation(run.ctx, run.session.ID, nil)
	if err != nil {
		return err
	}
	text, marker := original, "ORIGINAL-REPLY"
	if edited {
		text, marker = "Without tools, reply exactly EDITED-REPLY.", "EDITED-REPLY"
	}
	output, err := run.command(run.ctx, "replace_turn", protocol.CommandReplaceTurnPayload{Session: run.session.ID, Turn: first.run.Turn, ExpectedHead: first.run.Turn, ExpectedHistoryRevision: history.Page.Revision, ExpectedRevision: run.session.Revision, Message: message(text)})
	if err != nil {
		return err
	}
	value, err := outputAs[protocol.OutputTurnReplacedPayload](output, "turn_replaced")
	if err != nil {
		return err
	}
	if value.History == nil || value.History.Backup == nil || value.Turn.ID == first.run.Turn {
		return fail("Turn replacement did not preserve a distinct backup history")
	}
	if err := run.own(*value.History.Backup); err != nil {
		return err
	}
	if _, err := run.recordTurn(protocol.Output{Kind: "queued_turn", Data: value.Turn}, "queued_turn"); err != nil {
		return err
	}
	view, err := run.wait(value.Turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Replacement model turn did not complete")
	}
	if err := run.assertCapture(value.Turn); err != nil {
		return err
	}
	if err := run.requireText(view, marker); err != nil {
		return err
	}
	backup, err := run.controller.ReadConversation(run.ctx, value.History.Backup.ID, nil)
	if err != nil {
		return err
	}
	if !slices.ContainsFunc(backup.Page.Runs, func(value protocol.ConversationRun) bool { return value.Turn == first.run.Turn }) {
		return fail("Replacement backup lost the original turn")
	}
	run.evidence("backup", value.History.Backup.ID.String())
	return nil
}

func caseFork(run *caseRun) error {
	turns, err := run.branchFixture()
	if err != nil {
		return err
	}
	if err := run.refreshSession(); err != nil {
		return err
	}
	output, err := run.command(run.ctx, "fork_conversation", protocol.CommandForkConversationPayload{Session: run.session.ID, Through: turns[1].ID, ExpectedRevision: run.session.Revision})
	if err != nil {
		return err
	}
	fork, err := outputAs[protocol.Session](output, "session")
	if err != nil {
		return err
	}
	if err := run.own(fork); err != nil {
		return err
	}
	branch, err := run.controller.ReadConversation(run.ctx, fork.ID, nil)
	if err != nil {
		return err
	}
	original, err := run.controller.ReadConversation(run.ctx, run.session.ID, nil)
	if err != nil {
		return err
	}
	if fork.ID == run.session.ID || len(branch.Page.Runs) != 2 || len(original.Page.Runs) != 3 || slices.ContainsFunc(branch.Page.Runs, func(value protocol.ConversationRun) bool { return value.Turn == turns[2].ID }) {
		return fail("Fork boundary changed the source or included a later turn")
	}
	run.evidence("fork", fork.ID.String()+" preserves exactly two source turns")
	return nil
}

func caseRewind(run *caseRun) error {
	turns, err := run.branchFixture()
	if err != nil {
		return err
	}
	history, err := run.controller.ReadConversation(run.ctx, run.session.ID, nil)
	if err != nil {
		return err
	}
	output, err := run.command(run.ctx, "rewind_conversation", protocol.CommandRewindConversationPayload{Session: run.session.ID, Through: &turns[0].ID, ExpectedHead: turns[2].ID, ExpectedRevision: history.Page.Revision})
	if err != nil {
		return err
	}
	rewound, err := outputAs[protocol.ConversationRewind](output, "rewound")
	if err != nil {
		return err
	}
	if rewound.Backup == nil {
		return fail("Rewind did not preserve its original history in a backup")
	}
	if err := run.own(*rewound.Backup); err != nil {
		return err
	}
	current, err := run.controller.ReadConversation(run.ctx, run.session.ID, nil)
	if err != nil {
		return err
	}
	backup, err := run.controller.ReadConversation(run.ctx, rewound.Backup.ID, nil)
	if err != nil {
		return err
	}
	if len(current.Page.Runs) != 1 || current.Page.Runs[0].Turn != turns[0].ID || len(backup.Page.Runs) != 3 {
		return fail("Rewind or backup contains the wrong canonical turn range")
	}
	run.evidence("rewind", "One visible source turn; all three original turns retained in backup")
	return nil
}
