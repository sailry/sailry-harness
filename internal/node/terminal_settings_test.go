package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestTerminalSettings covers the saved shell and environment of new
// terminals: validation, revisions, the snapshot revision and discovery. It
// starts no terminal process.
// Over 1 s: each Node start installs and verifies the bundled packages.
func TestTerminalSettings(t *testing.T) {
	t.Parallel()
	node := moduleFixtureNode(t, "terminal")
	save := func(settings protocol.TerminalSettings) link.Response {
		return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_terminal_settings", Data: settings}))
	}
	read := func() protocol.TerminalSettings {
		output := commandFixture(t, node, protocol.Command{Kind: "read_terminal_settings"})
		if output.Kind != "terminal_settings" {
			t.Fatalf("read output = %#v", output)
		}
		return output.Data.(protocol.TerminalSettings)
	}
	if initial := read(); initial.Revision != 0 || initial.Shell != "" || len(initial.ShellArguments) != 0 || len(initial.Environment) != 0 {
		t.Fatalf("default settings = %#v", initial)
	}

	shell := filepath.Join(t.TempDir(), "shell")
	for _, test := range []struct {
		name     string
		settings protocol.TerminalSettings
		message  string
	}{
		{"control character in shell", protocol.TerminalSettings{Shell: shell + "\n"}, "invalid terminal shell path"},
		{"oversized shell", protocol.TerminalSettings{Shell: shell + strings.Repeat("a", 4097)}, "invalid terminal shell path"},
		{"arguments without shell", protocol.TerminalSettings{ShellArguments: []string{"-l"}}, "terminal shell arguments require a shell path"},
		{"too many arguments", protocol.TerminalSettings{Shell: shell, ShellArguments: slices.Repeat([]string{"a"}, 129)}, "invalid terminal shell arguments"},
		{"oversized arguments", protocol.TerminalSettings{Shell: shell, ShellArguments: []string{strings.Repeat("a", 64*1024+1)}}, "invalid terminal shell arguments"},
		{"null byte in argument", protocol.TerminalSettings{Shell: shell, ShellArguments: []string{"a\x00b"}}, "invalid terminal shell arguments"},
		{"relative shell", protocol.TerminalSettings{Shell: "zsh"}, "terminal shell must be an absolute path on the execution Node"},
		{"empty name", protocol.TerminalSettings{Environment: map[string]string{"": "1"}}, "invalid or reserved terminal environment name"},
		{"padded name", protocol.TerminalSettings{Environment: map[string]string{" EDITOR": "1"}}, "invalid or reserved terminal environment name"},
		{"assignment in name", protocol.TerminalSettings{Environment: map[string]string{"A=B": "1"}}, "invalid or reserved terminal environment name"},
		{"control character in name", protocol.TerminalSettings{Environment: map[string]string{"A\tB": "1"}}, "invalid or reserved terminal environment name"},
		{"reserved prefix", protocol.TerminalSettings{Environment: map[string]string{"sailry_token": "1"}}, "invalid or reserved terminal environment name"},
		{"loader prefix", protocol.TerminalSettings{Environment: map[string]string{"DYLD_INSERT_LIBRARIES": "1"}}, "invalid or reserved terminal environment name"},
		{"preload suffix", protocol.TerminalSettings{Environment: map[string]string{"custom_preload": "1"}}, "invalid or reserved terminal environment name"},
		{"reserved name", protocol.TerminalSettings{Environment: map[string]string{"Term": "dumb"}}, "invalid or reserved terminal environment name"},
		{"null byte in value", protocol.TerminalSettings{Environment: map[string]string{"EDITOR": "a\x00b"}}, "terminal environment value contains a null byte"},
		{"oversized environment", protocol.TerminalSettings{Environment: map[string]string{"EDITOR": strings.Repeat("a", 64*1024)}}, "terminal environment exceeds the supported size"},
	} {
		response := save(test.settings)
		if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeInvalidRequest || response.Fault.Message != test.message {
			t.Fatalf("%s = %#v", test.name, response)
		}
	}
	if after := read(); after.Revision != 0 {
		t.Fatalf("rejected settings were stored: %#v", after)
	}

	// The tool directory comes only from the saved environment.
	tools := t.TempDir()
	if err := os.WriteFile(filepath.Join(tools, "codex"), []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	wanted := protocol.TerminalSettings{Shell: shell, ShellArguments: []string{"--login"}, Environment: map[string]string{"EDITOR": "vi", "PATH": tools}}
	response := save(wanted)
	if response.Fault != nil || response.Output.Kind != "terminal_settings" {
		t.Fatalf("save = %#v", response)
	}
	saved := response.Output.Data.(protocol.TerminalSettings)
	if saved.Revision != 1 || saved.Shell != shell || !slices.Equal(saved.ShellArguments, wanted.ShellArguments) || saved.Environment["EDITOR"] != "vi" {
		t.Fatalf("saved settings = %#v", saved)
	}
	if stored := read(); stored.Revision != 1 || stored.Shell != shell || stored.Environment["PATH"] != tools {
		t.Fatalf("stored settings = %#v", stored)
	}
	if revision := snapshotFixture(t, node).TerminalSettingsRevision; revision != 1 {
		t.Fatalf("snapshot settings revision = %d", revision)
	}
	stale := save(wanted)
	if stale.Fault == nil || stale.Fault.Code != protocol.ErrorCodeRevisionConflict || stale.Fault.Message != "terminal settings changed" {
		t.Fatalf("stale save = %#v", stale)
	}

	if runtime.GOOS != "windows" {
		session := sessionFixture(t, node)
		output := commandFixture(t, node, protocol.Command{Kind: "list_terminal_tools", Data: protocol.CommandListTerminalToolsPayload{Worktree: session.Worktree}})
		available := []protocol.TerminalToolsTool{}
		for _, tool := range output.Data.([]protocol.TerminalToolsToolInfo) {
			if tool.Available {
				available = append(available, tool.Tool)
			}
		}
		if output.Kind != "terminal_tools" || !slices.Equal(available, []protocol.TerminalToolsTool{protocol.TerminalToolsToolCodex}) {
			t.Fatalf("tools with the saved PATH = %#v", output)
		}
	}

	output := commandFixture(t, node, protocol.Command{Kind: "list_shells"})
	shells, valid := output.Data.([]protocol.TerminalShell)
	if output.Kind != "shells" || !valid {
		t.Fatalf("shells output = %#v", output)
	}
	if !slices.IsSortedFunc(shells, func(first, second protocol.TerminalShell) int { return strings.Compare(first.Path, second.Path) }) {
		t.Fatalf("shells are not ordered: %#v", shells)
	}
	for _, found := range shells {
		if !filepath.IsAbs(found.Path) || found.Arguments == nil {
			t.Fatalf("shell = %#v", found)
		}
	}
	if runtime.GOOS != "windows" && !slices.ContainsFunc(shells, func(found protocol.TerminalShell) bool { return found.Path == "/bin/sh" }) {
		t.Fatalf("the system shell is missing: %#v", shells)
	}
}

// TestTerminalInventory keeps final events observable without retaining ended
// terminals in snapshots or accepting late running command results.
// Over 1 s: each Node start installs and verifies the bundled packages.
func TestTerminalInventory(t *testing.T) {
	t.Parallel()
	node := moduleFixtureNode(t, "terminal", "ssh")
	session := sessionFixture(t, node)
	ctx, cancel := context.WithTimeout(t.Context(), time.Minute)
	defer cancel()
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	if initial, err := subscription.Next(ctx); err != nil || initial.Kind != "snapshot" {
		t.Fatalf("initial = %#v %v", initial, err)
	}
	changed := func(id protocol.TerminalID) protocol.TerminalInfo {
		t.Helper()
		for {
			update, err := subscription.Next(ctx)
			if err != nil || update.Kind != "event" {
				t.Fatalf("update = %#v %v", update, err)
			}
			event := update.Data.(protocol.EventEnvelope).Event
			if event.Kind != "terminal_changed" {
				continue
			}
			body, err := json.Marshal(event.Data)
			if err != nil {
				t.Fatal(err)
			}
			var info protocol.TerminalInfo
			if err := json.Unmarshal(body, &info); err != nil {
				t.Fatal(err)
			}
			if info.ID == id {
				return info
			}
		}
	}
	finals := []protocol.TerminalStatus{
		{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 3}},
		{Kind: "stopped"},
		{Kind: "closed"},
		{Kind: "failed", Data: protocol.TerminalStatusFailedPayload{Message: "SSH terminal ended without an exit status"}},
	}
	for _, ssh := range []bool{false, true} {
		for _, final := range finals {
			owner, title, access := node.ID(), "", protocol.ProcessAccessUnrestricted
			directory, profile := t.TempDir(), protocol.NewSshID()
			running := protocol.TerminalInfo{ID: protocol.NewTerminalID(), Worktree: &session.Worktree, Access: &access, Status: protocol.TerminalStatus{Kind: "running"}, Title: &title, Directory: &directory, Owner: &owner, Revision: 1}
			if ssh {
				running.Worktree, running.Access, running.Directory, running.Ssh = nil, nil, nil, &profile
			}
			node.terminalChangedBy(node.ctx, running)
			if info := changed(running.ID); info.Status.Kind != "running" || info.Owner == nil {
				t.Fatalf("running event = %#v", info)
			}
			if values := snapshotFixture(t, node).Terminals; len(values) != 1 || values[0].ID != running.ID {
				t.Fatalf("running inventory = %#v", values)
			}
			ended := running
			ended.Status, ended.Owner, ended.Revision = final, nil, 2
			node.terminalChangedBy(node.ctx, ended)
			if info := changed(running.ID); info.Status.Kind != final.Kind || info.Owner != nil || info.Revision != 2 {
				t.Fatalf("final event = %#v", info)
			}
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				for _, event := range []bool{false, true} {
					for _, revision := range []uint64{1, 2, 3} {
						running.Revision = revision
						changes, err := store.SaveTerminal(ctx, db, running, event)
						if err != nil {
							return err
						}
						if len(changes) != 0 {
							return fmt.Errorf("late running update resurrected %s: %#v", final.Kind, changes)
						}
					}
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			if values := snapshotFixture(t, node).Terminals; len(values) != 0 {
				t.Fatalf("ended inventory = %#v", values)
			}
			local := commandFixture(t, node, protocol.Command{Kind: "list_terminals", Data: protocol.CommandListTerminalsPayload{Worktree: session.Worktree}})
			remote := commandFixture(t, node, protocol.Command{Kind: "list_ssh_terminals", Data: protocol.CommandListSshTerminalsPayload{Profile: profile}})
			if len(local.Data.([]protocol.TerminalInfo)) != 0 || len(remote.Data.([]protocol.TerminalInfo)) != 0 {
				t.Fatalf("ended terminal remained listed: %#v %#v", local, remote)
			}
			command := protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: session.Worktree, Terminal: running.ID}}
			if ssh {
				command = protocol.Command{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: running.ID}}
			}
			response := dispatchFixture(t, node, owner, protocol.NewRequest(owner, command))
			if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeNotFound {
				t.Fatalf("close ended terminal = %#v", response)
			}
		}
	}
}

// TestTerminalRecovery retires records from the preceding process without
// changing the original request's durable result or replaying its launch.
func TestTerminalRecovery(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	owner := protocol.NodeID{1}
	storage, err := store.Open(ctx, t.TempDir(), owner)
	if err != nil {
		t.Fatal(err)
	}
	defer storage.Close()
	launch := protocol.DefaultTerminalLaunch()
	launch.Access = protocol.ProcessAccessUnrestricted
	launch.Appearance.ColorScheme = protocol.TerminalColorSchemeDark
	request := protocol.NewRequest(owner, protocol.Command{Kind: "create_terminal", Data: launch})
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	original := protocol.TerminalInfo{ID: protocol.TerminalID(request.ID), Status: protocol.TerminalStatus{Kind: "running"}, Owner: &owner, Revision: 1}
	output := protocol.Output{Kind: "terminal", Data: original}
	result, err := json.Marshal(protocol.Result[protocol.Output, protocol.Fault]{Ok: &output})
	if err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)", owner[:], request.ID.String(), body, string(result)); err != nil {
			return err
		}
		for _, status := range []protocol.TerminalStatus{original.Status, {Kind: "stopped"}, {Kind: "closed"}, {Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 3}}, {Kind: "failed", Data: protocol.TerminalStatusFailedPayload{Message: "interrupted"}}} {
			info := original
			info.Status = status
			if status.Kind != "running" {
				info.ID = protocol.NewTerminalID()
			}
			encoded, err := json.Marshal(info)
			if err != nil {
				return err
			}
			// Legacy rows include final statuses; SaveTerminal no longer retains them.
			if _, err := db.ExecContext(ctx, "INSERT INTO terminals(id,body) VALUES(?1,?2)", info.ID.String(), encoded); err != nil {
				return err
			}
		}
		return store.RecoverTerminals(ctx, db)
	}); err != nil {
		t.Fatal(err)
	}
	var cursor uint64
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM terminals").Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			t.Fatalf("recovered terminal rows = %d", count)
		}
		var status string
		var savedBody, savedResult []byte
		if err := db.QueryRowContext(ctx, "SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2", owner[:], request.ID.String()).Scan(&savedBody, &status, &savedResult); err != nil {
			return err
		}
		if status != "completed" || !bytes.Equal(body, savedBody) || !bytes.Equal(result, savedResult) {
			t.Fatalf("original request changed: %s %s %s", status, savedBody, savedResult)
		}
		var events []protocol.EventEnvelope
		var err error
		cursor, events, err = store.EventPage(ctx, db, owner, 0, 0)
		if err != nil {
			return err
		}
		if len(events) != 5 {
			t.Fatalf("recovery events = %#v", events)
		}
		for _, event := range events {
			info := event.Event.Data.(*protocol.TerminalInfo)
			if event.Event.Kind != "terminal_changed" || info.Owner != nil || info.Status.Kind == "running" {
				t.Fatalf("recovery event = %#v", event)
			}
			if info.ID == original.ID && (info.Status.Kind != "stopped" || info.Revision != 2) {
				t.Fatalf("interrupted terminal = %#v", info)
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error { return store.RecoverTerminals(ctx, db) }); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		_, events, err := store.EventPage(ctx, db, owner, cursor, 0)
		if len(events) != 0 {
			t.Fatalf("repeat recovery emitted events: %#v", events)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
