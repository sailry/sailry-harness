package node

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestTerminalSettings covers the saved shell and environment of new
// terminals: validation, revisions, the snapshot revision and discovery. It
// starts no terminal process.
func TestTerminalSettings(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
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

// TestTerminalInventory covers what controllers see of terminals that no
// longer run: they stay in the snapshot with their final status until a close
// command marks them closed. It starts no terminal process or connection.
func TestTerminalInventory(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	session := sessionFixture(t, node)
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	if initial, err := subscription.Next(ctx); err != nil || initial.Kind != "snapshot" {
		t.Fatalf("initial = %#v %v", initial, err)
	}
	// changed waits for the next terminal_changed event of one terminal.
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
	stored := func(id protocol.TerminalID) protocol.TerminalInfo {
		t.Helper()
		for _, info := range snapshotFixture(t, node).Terminals {
			if info.ID == id {
				return info
			}
		}
		t.Fatalf("terminal %s is not in the snapshot", id)
		return protocol.TerminalInfo{}
	}
	fails := func(command protocol.Command, code protocol.ErrorCode, message string) {
		t.Helper()
		response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
		if response.Fault == nil || response.Fault.Code != code || response.Fault.Message != message {
			t.Fatalf("%s = %#v", command.Kind, response)
		}
	}

	owner, title, access := node.ID(), "", protocol.ProcessAccessUnrestricted
	directory := t.TempDir()
	local := protocol.TerminalInfo{ID: protocol.NewTerminalID(), Worktree: &session.Worktree, Access: &access, Status: protocol.TerminalStatus{Kind: "running"}, Title: &title, Directory: &directory, Owner: &owner, Revision: 1}
	node.terminalChanged(local)
	if info := changed(local.ID); info.Status.Kind != "running" || info.Owner == nil {
		t.Fatalf("running event = %#v", info)
	}
	// A running record this process does not run cannot be closed from storage.
	fails(protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: session.Worktree, Terminal: local.ID}}, protocol.ErrorCodeNotFound, "terminal is not running on this Node")

	local.Status, local.Owner, local.Revision = protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 3}}, nil, 2
	node.terminalChanged(local)
	if info := changed(local.ID); info.Status.Kind != "exited" || info.Owner != nil || info.Revision != 2 {
		t.Fatalf("exit event = %#v", info)
	}
	if info := stored(local.ID); info.Status.Kind != "exited" {
		t.Fatalf("exited terminal in the snapshot = %#v", info)
	}
	listed := commandFixture(t, node, protocol.Command{Kind: "list_terminals", Data: protocol.CommandListTerminalsPayload{Worktree: session.Worktree}}).Data.([]protocol.TerminalInfo)
	if len(listed) != 1 || listed[0].ID != local.ID || listed[0].Status.Kind != "exited" {
		t.Fatalf("listed terminals = %#v", listed)
	}
	fails(protocol.Command{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: local.ID}}, protocol.ErrorCodeNotFound, "terminal is not running on this Node")

	output := commandFixture(t, node, protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: session.Worktree, Terminal: local.ID}})
	if closed := output.Data.(protocol.TerminalInfo); output.Kind != "terminal" || closed.Status.Kind != "closed" || closed.Owner != nil || closed.Revision != 3 {
		t.Fatalf("close output = %#v", output)
	}
	if info := changed(local.ID); info.Status.Kind != "closed" || info.Revision != 3 {
		t.Fatalf("close event = %#v", info)
	}
	if info := stored(local.ID); info.Status.Kind != "closed" {
		t.Fatalf("closed terminal in the snapshot = %#v", info)
	}
	appearance := protocol.DefaultTerminalAppearance()
	appearance.ColorScheme = protocol.TerminalColorSchemeLight
	viewport := protocol.TerminalViewport{Columns: 80, Rows: 24}
	fails(protocol.Command{Kind: "open_terminal", Data: protocol.CommandOpenTerminalPayload{Terminal: local.ID, Viewport: viewport, Appearance: appearance}}, protocol.ErrorCodeNotFound, "terminal has been closed")

	profile := protocol.NewSshID()
	remote := protocol.TerminalInfo{ID: protocol.NewTerminalID(), Ssh: &profile, Status: protocol.TerminalStatus{Kind: "stopped"}, Title: &title, Owner: &owner, Revision: 1}
	unowned := protocol.TerminalInfo{ID: protocol.NewTerminalID(), Ssh: &profile, Status: protocol.TerminalStatus{Kind: "failed", Data: protocol.TerminalStatusFailedPayload{Message: "SSH terminal ended without an exit status"}}, Title: &title, Revision: 1}
	elsewhere := protocol.NewSshID()
	other := protocol.TerminalInfo{ID: protocol.NewTerminalID(), Ssh: &elsewhere, Status: protocol.TerminalStatus{Kind: "stopped"}, Title: &title, Owner: &owner, Revision: 1}
	for _, info := range []protocol.TerminalInfo{remote, unowned, other} {
		node.terminalChanged(info)
		if event := changed(info.ID); event.Ssh == nil || event.Worktree != nil || event.Status.Kind != info.Status.Kind {
			t.Fatalf("SSH terminal event = %#v", event)
		}
	}
	list := func() []protocol.TerminalInfo {
		t.Helper()
		output := commandFixture(t, node, protocol.Command{Kind: "list_ssh_terminals", Data: protocol.CommandListSshTerminalsPayload{Profile: profile}})
		if output.Kind != "terminals" {
			t.Fatalf("SSH terminals output = %#v", output)
		}
		return output.Data.([]protocol.TerminalInfo)
	}
	if values := list(); len(values) != 1 || values[0].ID != remote.ID {
		t.Fatalf("SSH terminals of the caller = %#v", values)
	}
	fails(protocol.Command{Kind: "open_terminal", Data: protocol.CommandOpenTerminalPayload{Terminal: remote.ID, Viewport: viewport, Appearance: appearance}}, protocol.ErrorCodeConflict, "terminal requires an explicit new launch")
	fails(protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: session.Worktree, Terminal: remote.ID}}, protocol.ErrorCodeNotFound, "terminal is not running on this Node")
	output = commandFixture(t, node, protocol.Command{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: remote.ID}})
	if closed := output.Data.(protocol.TerminalInfo); output.Kind != "terminal" || closed.Status.Kind != "closed" || closed.Owner != nil || closed.Revision != 2 {
		t.Fatalf("SSH close output = %#v", output)
	}
	if info := changed(remote.ID); info.Status.Kind != "closed" || info.Ssh == nil {
		t.Fatalf("SSH close event = %#v", info)
	}
	if values := list(); len(values) != 0 {
		t.Fatalf("closed SSH terminal is still listed: %#v", values)
	}
	fails(protocol.Command{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: protocol.NewTerminalID()}}, protocol.ErrorCodeNotFound, "terminal is not running on this Node")
	fails(protocol.Command{Kind: "open_ssh_terminal", Data: protocol.CommandOpenSshTerminalPayload{Profile: profile, ExpectedRevision: 1, Launch: protocol.SshTerminalLaunch{Viewport: protocol.TerminalViewport{Columns: 501, Rows: 24}, Appearance: appearance}}}, protocol.ErrorCodeInvalidRequest, "terminal viewport is outside the supported range")
}
