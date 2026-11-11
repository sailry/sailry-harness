//go:build integration

package node

import (
	"testing"

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
