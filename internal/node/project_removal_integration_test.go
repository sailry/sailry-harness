//go:build integration

package node

import (
	"context"
	"database/sql"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A real shell runs in the project while it is removed.
func TestProjectRemovalClosesLiveTerminal(t *testing.T) {
	node := nodeFixture(t)
	project, worktree := projectFixture(t, node)
	launch := protocol.TerminalLaunch{Worktree: worktree, Access: protocol.ProcessAccessUnrestricted, Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24}, Appearance: protocol.TerminalAppearance{ColorScheme: protocol.TerminalColorSchemeLight}}
	opened, valid := dispatchPayload[protocol.TerminalInfo](commandFixture(t, node, protocol.Command{Kind: "create_terminal", Data: launch}).Data)
	if !valid || opened.Status.Kind != "running" || len(node.terminals.List(worktree)) != 1 {
		t.Fatalf("terminal = %#v", opened)
	}
	remove := func(expected protocol.Project) *protocol.Fault {
		return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: expected}})).Fault
	}
	stale := project
	stale.Name = "Stale"
	if fault := remove(stale); fault == nil || fault.Code != protocol.ErrorCodeRevisionConflict || fault.Details != nil {
		t.Fatalf("stale removal = %#v", fault)
	}
	if len(node.terminals.List(worktree)) != 1 {
		t.Fatal("a refused removal closed the project's terminal")
	}
	if fault := remove(project); fault != nil {
		t.Fatal("an open terminal refused the removal its confirmation promises:", fault)
	}
	if len(snapshotFixture(t, node).Projects) != 0 || len(node.terminals.List(worktree)) != 0 {
		t.Fatal("removal kept the project or its terminal")
	}
	if closed := terminalFixture(t, node, opened.ID); closed.Status.Kind != "closed" {
		t.Fatalf("terminal record = %#v", closed)
	}
}

// A real shell starts while the store refuses to record its result: the
// terminal runs, so the request ends as an outcome to inspect and never as
// a request that was not applied.
func TestTerminalResultUnrecorded(t *testing.T) {
	node := nodeFixture(t)
	_, worktree := projectFixture(t, node)
	ctx := context.Background()
	gate := func(statement string) {
		t.Helper()
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, statement)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	gate("CREATE TEMP TABLE result_gate AS SELECT 1 AS closed")
	gate("CREATE TEMP TRIGGER result_refused BEFORE UPDATE OF status ON requests WHEN (SELECT closed FROM result_gate)=1 BEGIN SELECT RAISE(ABORT,'result write refused'); END")
	defer gate("DROP TRIGGER result_refused")
	launch := protocol.TerminalLaunch{Worktree: worktree, Access: protocol.ProcessAccessUnrestricted, Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24}, Appearance: protocol.TerminalAppearance{ColorScheme: protocol.TerminalColorSchemeLight}}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "create_terminal", Data: launch})
	accepted, err := node.Dispatch(ctx, node.ID(), request)
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(30 * time.Second)
	for len(node.terminals.List(worktree)) != 1 {
		if time.Now().After(deadline) {
			t.Fatal("the terminal did not start")
		}
		time.Sleep(20 * time.Millisecond)
	}
	gate("UPDATE result_gate SET closed=0")
	result := <-accepted.Completion
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
		t.Fatalf("a started terminal whose row was not stored = %#v, %v", result.Output, result.Fault)
	}
	replay := dispatchFixture(t, node, node.ID(), request)
	if replay.Fault == nil || *replay.Fault != *result.Fault || len(node.terminals.List(worktree)) != 1 {
		t.Fatalf("replay = %v, %d terminals", replay.Fault, len(node.terminals.List(worktree)))
	}
}
