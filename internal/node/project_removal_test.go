package node

import (
	"context"
	"database/sql"
	"testing"
	"time"

	git "github.com/go-git/go-git/v5"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// projectFixture registers a Git directory and returns it with its main worktree.
func projectFixture(t *testing.T, node *Node) (protocol.Project, protocol.WorktreeID) {
	t.Helper()
	directory := t.TempDir()
	if _, err := git.PlainInit(directory, false); err != nil {
		t.Fatal(err)
	}
	draft := protocol.ProjectsDraft{Name: "Project", Path: directory, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project)
	for _, tree := range snapshotFixture(t, node).Worktrees {
		if tree.Project != nil && *tree.Project == project.ID && tree.Main {
			return project, tree.ID
		}
	}
	t.Fatal("project has no main worktree")
	return project, protocol.WorktreeID{}
}

func terminalFixture(t *testing.T, node *Node, id protocol.TerminalID) protocol.TerminalInfo {
	t.Helper()
	for _, terminal := range snapshotFixture(t, node).Terminals {
		if terminal.ID == id {
			return terminal
		}
	}
	t.Fatal("terminal record is missing")
	return protocol.TerminalInfo{}
}

func TestProjectRemovalClosesResources(t *testing.T) {
	record := func(t *testing.T, node *Node, worktree protocol.WorktreeID, status string) protocol.TerminalID {
		t.Helper()
		terminal := protocol.DefaultTerminalInfo()
		terminal.ID, terminal.Worktree, terminal.Revision = protocol.TerminalID(protocol.NewRequestID()), &worktree, 1
		terminal.Status.Kind = status
		if err := node.profile.Store.Node.Write(context.Background(), func(db *sql.Conn) error {
			_, err := store.SaveTerminal(context.Background(), db, terminal, false)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return terminal.ID
	}
	remove := func(t *testing.T, node *Node, expected protocol.Project) *protocol.Fault {
		t.Helper()
		return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: expected}})).Fault
	}
	t.Run("a_terminal_left_by_an_earlier_process_closes_with_the_project", func(t *testing.T) {
		node := nodeFixture(t)
		project, worktree := projectFixture(t, node)
		terminal := record(t, node, worktree, "stopped")
		if fault := remove(t, node, project); fault != nil {
			t.Fatal("a stopped terminal refused the removal its confirmation promises:", fault)
		}
		if len(snapshotFixture(t, node).Projects) != 0 {
			t.Fatal("removed project remained visible")
		}
		if closed := terminalFixture(t, node, terminal); closed.Status.Kind != "closed" || closed.Revision != 2 {
			t.Fatalf("terminal record = %#v", closed)
		}
	})
	t.Run("a_terminal_that_could_not_be_closed_keeps_the_project", func(t *testing.T) {
		node := nodeFixture(t)
		project, worktree := projectFixture(t, node)
		// Running in the record with no terminal in this process: closing it
		// here would hide a terminal another owner may still serve.
		terminal := record(t, node, worktree, "running")
		fault := remove(t, node, project)
		if fault == nil || fault.Code != protocol.ErrorCodeBusy || fault.Details != nil {
			t.Fatalf("removal = %#v", fault)
		}
		if len(snapshotFixture(t, node).Projects) != 1 || terminalFixture(t, node, terminal).Status.Kind != "running" {
			t.Fatal("a refused removal changed the project or its terminal")
		}
	})
	t.Run("background_commands_of_the_project_stop", func(t *testing.T) {
		node := nodeFixture(t)
		project, _ := projectFixture(t, node)
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = protocol.NewProviderID(), "fixture", protocol.WorkModeCode, protocol.PermissionProject
		inside := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
		outside := sessionFixture(t, node)
		for _, session := range []protocol.SessionID{inside.ID, outside.ID} {
			if _, err := node.processes.Start(process.Launch{ID: protocol.NewRequestID(), Session: session, Turn: protocol.NewTurnID(), Root: project.Path, Command: "sleep 60", Access: protocol.ProcessAccessUnrestricted, TimeoutMs: 60_000}); err != nil {
				t.Fatal(err)
			}
		}
		if fault := remove(t, node, project); fault != nil {
			t.Fatal(fault)
		}
		deadline := time.Now().Add(20 * time.Second)
		for {
			items := node.processes.List(inside.ID)
			if len(items) == 1 && items[0].Status.Kind != "running" && items[0].Status.Kind != "stopping" {
				break
			}
			if time.Now().After(deadline) {
				t.Fatalf("project removal kept its background command: %#v", items)
			}
			time.Sleep(20 * time.Millisecond)
		}
		if items := node.processes.List(outside.ID); len(items) != 1 || items[0].Status.Kind != "running" {
			t.Fatalf("removal stopped a command outside the project: %#v", items)
		}
	})
}
