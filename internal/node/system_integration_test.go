//go:build integration

package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// computerHelperName is the helper's path inside a helpers directory.
func computerHelperName() string {
	switch runtime.GOOS {
	case "darwin":
		return filepath.Join("Sailry Computer Use.app", "Contents", "MacOS", "agent-desktop")
	case "windows":
		return "agent-desktop.exe"
	}
	return "agent-desktop"
}

// buildComputerHelper compiles the stand-in helper of testdata/computerhelper
// into a new helpers directory and returns that directory and the helper.
func buildComputerHelper(t *testing.T) (string, string) {
	t.Helper()
	helpers := filepath.Join(t.TempDir(), "helpers")
	helper := filepath.Join(helpers, computerHelperName())
	if err := os.MkdirAll(filepath.Dir(helper), 0o700); err != nil {
		t.Fatal(err)
	}
	build := exec.CommandContext(t.Context(), "go", "build", "-o", helper, "./testdata/computerhelper")
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("building the stand-in helper: %v\n%s", err, output)
	}
	return helpers, helper
}

// helperCalls are the lines the stand-in helper recorded, optionally only
// those with a prefix.
func helperCalls(t *testing.T, helper, prefix string) []string {
	t.Helper()
	body, err := os.ReadFile(filepath.Join(filepath.Dir(helper), "calls.log"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		t.Fatal(err)
	}
	var lines []string
	for _, line := range strings.Split(strings.TrimSpace(string(body)), "\n") {
		if line != "" && strings.HasPrefix(line, prefix) {
			lines = append(lines, line)
		}
	}
	return lines
}

func helperGrants(t *testing.T, helper, accessibility, screen string) {
	t.Helper()
	body, _ := json.Marshal(map[string]string{"accessibility": accessibility, "screen_recording": screen})
	if err := os.WriteFile(filepath.Join(filepath.Dir(helper), "state.json"), body, 0o600); err != nil {
		t.Fatal(err)
	}
}

// computerScript is the scripted model of the computer turn: it selects the
// helper's two tools, reads, controls and answers.
type computerScript struct {
	fantasy.LanguageModel
	read, control string
	step          atomic.Int32
	offered       atomic.Value // []string of the tools offered at step 2
}

func (script *computerScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	step := script.step.Add(1)
	if step > 4 {
		return nil, errors.New("unexpected extra model request")
	}
	if step == 2 {
		var names []string
		for _, tool := range call.Tools {
			names = append(names, tool.GetName())
		}
		script.offered.Store(names)
	}
	return func(yield func(fantasy.StreamPart) bool) {
		part := fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall}
		switch step {
		case 1:
			arguments, _ := json.Marshal(map[string]any{"query": "select:" + script.read + "," + script.control})
			part.ID, part.ToolCallName, part.ToolCallInput = "discover", "tool_search", string(arguments)
		case 2:
			part.ID, part.ToolCallName, part.ToolCallInput = "read-call", script.read, `{"ref_id":"window-1"}`
		case 3:
			part.ID, part.ToolCallName, part.ToolCallInput = "control-call", script.control, `{"ref_id":"button-7"}`
		case 4:
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Used the computer tools"}
		}
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

// computerPackage installs and enables the bundled computer package.
func computerPackage(t *testing.T, node *Node) protocol.PluginInfo {
	t.Helper()
	read := func() (protocol.PluginInfo, bool) {
		if !slices.ContainsFunc(snapshotFixture(t, node).Plugins, func(entry protocol.PluginSummary) bool { return entry.Name == "computer" }) {
			return protocol.PluginInfo{}, false
		}
		return commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "computer"}}).Data.(protocol.PluginInfo), true
	}
	info, installed := read()
	if !installed {
		commandFixture(t, node, protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "computer"}})
		info, _ = read()
	}
	if !info.Summary.Enabled {
		commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "computer", ExpectedRevision: info.Summary.Revision, Enabled: true}})
		info, _ = read()
	}
	if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || len(info.Issues) != 0 {
		t.Fatalf("the bundled computer package is not usable: %+v, issues %+v", info.Summary, info.Issues)
	}
	return info
}

// TestSystemComputerIntegration is the B13 computer-use acceptance against a
// real Node and a stand-in helper executable built by the test: the locator,
// the two permission commands through Node.Dispatch, and an agent turn that
// is offered the helper's MCP tools, runs the read tool and waits for
// approval before the control tool. No real helper runs and nothing touches
// the screen, the pointer, the keyboard or the privacy settings.
func TestSystemComputerIntegration(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("the helper's grants are gated on macOS only")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 180*time.Second)
	defer cancel()
	helpers, helper := buildComputerHelper(t)
	unavailable := func(err error, message string) bool {
		var fault *protocol.Fault
		return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeUnavailable && fault.Message == message
	}

	// (1) The locator: only SAILRY_HELPERS_DIR or the directory beside the
	// Node executable; never PATH.
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	beside := filepath.Join(filepath.Dir(executable), "helpers")
	if _, err := os.Lstat(beside); err == nil {
		t.Fatalf("%s already exists", beside)
	}
	empty := t.TempDir()
	decoy := filepath.Join(t.TempDir(), "agent-desktop")
	marker := filepath.Join(filepath.Dir(decoy), "decoy-ran")
	if err := os.WriteFile(decoy, []byte("#!/bin/sh\ntouch '"+marker+"'\necho '{\"ok\":true,\"command\":\"permissions\",\"data\":{\"accessibility\":{\"state\":\"granted\"},\"screen_recording\":{\"state\":\"granted\"}}}'\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", filepath.Dir(decoy)+string(os.PathListSeparator)+filepath.Dir(helper)+string(os.PathListSeparator)+os.Getenv("PATH"))
	if found, err := exec.LookPath("agent-desktop"); err != nil || found != decoy {
		t.Fatalf("the decoy helper is not first on PATH: %q, %v", found, err)
	}
	t.Setenv("SAILRY_HELPERS_DIR", "")
	if path, err := computer.Executable(); !unavailable(err, "computer helper is missing") {
		t.Fatalf("a helper on PATH alone was located: %q, %v", path, err)
	}
	t.Setenv("SAILRY_HELPERS_DIR", empty)
	if path, err := computer.Executable(); !unavailable(err, "computer helper is missing") {
		t.Fatalf("an empty helpers directory located %q, %v", path, err)
	}
	t.Setenv("SAILRY_HELPERS_DIR", helpers)
	if path, err := computer.Executable(); err != nil || path != helper {
		t.Fatalf("SAILRY_HELPERS_DIR located %q, %v; want %q", path, err, helper)
	}
	// Beside the executable: a link to the built helper, removed at once.
	t.Setenv("SAILRY_HELPERS_DIR", "")
	if err := os.Symlink(helpers, beside); err != nil {
		t.Fatal(err)
	}
	path, err := computer.Executable()
	if removal := os.Remove(beside); removal != nil {
		t.Fatal(removal)
	}
	if err != nil || path != filepath.Join(beside, computerHelperName()) {
		t.Fatalf("the helpers directory beside the executable located %q, %v", path, err)
	}
	if mode := os.Chmod(helper, 0o600); mode != nil {
		t.Fatal(mode)
	}
	t.Setenv("SAILRY_HELPERS_DIR", helpers)
	if path, err := computer.Executable(); !unavailable(err, "computer helper is not executable") {
		t.Fatalf("a helper without the executable bit was located: %q, %v", path, err)
	}
	if mode := os.Chmod(helper, 0o700); mode != nil {
		t.Fatal(mode)
	}
	t.Log("step 1 passed: the helper is located through SAILRY_HELPERS_DIR and beside the executable only; PATH, an empty directory and a non-executable file are Unavailable")

	// A real Node with the real stdio MCP launcher and a scripted model.
	script := &computerScript{read: mcp.Alias("computer", "desktop", "desktop_snapshot"), control: mcp.Alias("computer", "desktop", "desktop_click")}
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	info := computerPackage(t, node)
	reference := plugins.PackageReference(info)
	scoped := func(command protocol.Command) protocol.Request {
		request := protocol.NewRequest(node.ID(), command)
		request.Plugin = &protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "permissions"}, Package: reference}
		return request
	}
	dispatch := func(caller protocol.NodeID, request protocol.Request) (protocol.ComputerPermissions, *protocol.Fault) {
		t.Helper()
		accepted, err := node.Dispatch(ctx, caller, request)
		if err != nil {
			var fault *protocol.Fault
			if !errors.As(err, &fault) {
				t.Fatal(err)
			}
			return protocol.ComputerPermissions{}, fault
		}
		select {
		case result, delivered := <-accepted.Completion:
			if !delivered {
				t.Fatal("the completion was lost")
			}
			if result.Fault != nil {
				return protocol.ComputerPermissions{}, result.Fault
			}
			if result.Output.Kind != "computer_permissions" {
				t.Fatalf("output kind %q", result.Output.Kind)
			}
			switch value := result.Output.Data.(type) {
			case protocol.ComputerPermissions:
				return value, nil
			case *protocol.ComputerPermissions:
				return *value, nil
			}
			t.Fatalf("output data %T", result.Output.Data)
		case <-time.After(20 * time.Second):
			// The helper's own deadline is 15 seconds.
			t.Fatal("the permission command did not complete within 20 seconds")
		}
		return protocol.ComputerPermissions{}, nil
	}
	read := protocol.Command{Kind: "read_computer_permissions"}
	request := func(permission protocol.ComputerPermission) protocol.Command {
		return protocol.Command{Kind: "request_computer_permission", Data: protocol.CommandRequestComputerPermissionPayload{Permission: permission}}
	}

	// (2) No helper: Unavailable, and nothing on PATH is used instead.
	t.Setenv("SAILRY_HELPERS_DIR", empty)
	for _, command := range []protocol.Command{read, request(protocol.ComputerPermissionAccessibility)} {
		if value, fault := dispatch(node.ID(), scoped(command)); fault == nil || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "computer helper is missing" {
			t.Fatalf("%s without a helper = %+v, %v", command.Kind, value, fault)
		}
	}
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the helper on PATH ran as a fallback: %v", err)
	}
	if calls := helperCalls(t, helper, ""); len(calls) != 0 {
		t.Fatalf("the unlocated helper ran: %v", calls)
	}
	// Without a captured helper configuration the command is not configured.
	if value, fault := dispatch(node.ID(), protocol.NewRequest(node.ID(), read)); fault == nil || fault.Code != protocol.ErrorCodeNotConfigured || fault.Message != "computer use requires a captured helper configuration" {
		t.Fatalf("read without a package = %+v, %v", value, fault)
	}
	t.Log("step 2 passed: with no helper both commands fault Unavailable \"computer helper is missing\"; the helper on PATH never ran")

	// (3) The helper's grants through read_computer_permissions.
	t.Setenv("SAILRY_HELPERS_DIR", helpers)
	granted := func(value *bool) string {
		if value == nil {
			return "nil"
		}
		return fmt.Sprint(*value)
	}
	for _, state := range []struct{ accessibility, screen, wantAccessibility, wantScreen string }{
		{"denied", "denied", "false", "false"},
		{"granted", "denied", "true", "false"},
		{"not_required", "unknown", "nil", "nil"},
		{"granted", "granted", "true", "true"},
	} {
		helperGrants(t, helper, state.accessibility, state.screen)
		value, fault := dispatch(node.ID(), scoped(read))
		if fault != nil || value.Node != node.ID() || !value.Local || value.Platform != "macos" ||
			granted(value.Accessibility) != state.wantAccessibility || granted(value.ScreenCapture) != state.wantScreen {
			t.Fatalf("read with %+v = %+v, %v", state, value, fault)
		}
	}
	helperGrants(t, helper, "malformed", "denied")
	if value, fault := dispatch(node.ID(), scoped(read)); fault == nil || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "computer permission diagnostics are malformed" {
		t.Fatalf("malformed diagnostics = %+v, %v", value, fault)
	}
	if calls := helperCalls(t, helper, "run "); len(calls) != 5 || slices.ContainsFunc(calls, func(line string) bool { return line != "run permissions" }) {
		t.Fatalf("helper invocations after five reads: %v", calls)
	}
	t.Log("step 3 passed: read_computer_permissions ran `permissions` once per read and mapped granted, denied, not required and unknown; malformed output is Unavailable")

	// (4) request_computer_permission: a granted permission does not ask;
	// a missing one runs `permissions --request` once and reads again; a
	// remote caller is refused before the helper runs.
	helperGrants(t, helper, "granted", "denied")
	before := len(helperCalls(t, helper, "run "))
	if value, fault := dispatch(node.ID(), scoped(request(protocol.ComputerPermissionAccessibility))); fault != nil || granted(value.Accessibility) != "true" || granted(value.ScreenCapture) != "false" {
		t.Fatalf("request of a granted permission = %+v, %v", value, fault)
	}
	if calls := helperCalls(t, helper, "run ")[before:]; !slices.Equal(calls, []string{"run permissions"}) {
		t.Fatalf("a granted permission ran %v", calls)
	}
	before = len(helperCalls(t, helper, "run "))
	if value, fault := dispatch(node.ID(), scoped(request(protocol.ComputerPermissionScreenCapture))); fault != nil || granted(value.Accessibility) != "true" || granted(value.ScreenCapture) != "true" {
		t.Fatalf("request of a missing permission = %+v, %v", value, fault)
	}
	if calls := helperCalls(t, helper, "run ")[before:]; !slices.Equal(calls, []string{"run permissions", "run permissions --request", "run permissions"}) {
		t.Fatalf("a missing permission ran %v", calls)
	}
	before = len(helperCalls(t, helper, "run "))
	remote := node.ID()
	remote[0] ^= 0xff
	if value, fault := dispatch(remote, protocol.NewRequest(node.ID(), request(protocol.ComputerPermissionAccessibility))); fault == nil || fault.Code != protocol.ErrorCodePermissionDenied || fault.Message != "system permissions must be granted on the execution device" {
		t.Fatalf("request from another device = %+v, %v", value, fault)
	}
	if calls := helperCalls(t, helper, "run ")[before:]; len(calls) != 0 {
		t.Fatalf("a remote request ran the helper: %v", calls)
	}
	t.Log("step 4 passed: request_computer_permission asked the helper exactly once for a missing grant, not at all for a granted one, and refused another device")

	// (5) A real agent turn with the computer package enabled.
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Computer acceptance", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Look at the window and press the button"}}})
	approvals := 0
	// The turn starts the helper and waits for one approval: 60 seconds
	// bound the whole loop, each wait for an event included.
	deadline, stop := context.WithTimeout(ctx, 60*time.Second)
	defer stop()
	for completed := false; !completed; {
		var history protocol.ConversationHistory
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(ctx, db, session.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State != protocol.ConversationApprovalStatePending {
				continue
			}
			approvals++
			// The read tool has run; the control tool has not.
			if calls := helperCalls(t, helper, "tool "); !slices.Equal(calls, []string{"tool desktop_snapshot window-1"}) {
				t.Fatalf("tool calls while the control tool waits for approval: %v", calls)
			}
			if script.step.Load() != 3 {
				t.Fatalf("the approval is pending at model step %d", script.step.Load())
			}
			commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("the computer turn ended as %+v; helper calls %v", run, helperCalls(t, helper, ""))
			}
		}
		if !completed {
			if _, err := subscription.Next(deadline); err != nil {
				t.Fatalf("the computer turn did not complete: %v; model step %d; helper calls %v", err, script.step.Load(), helperCalls(t, helper, ""))
			}
		}
	}
	offered, _ := script.offered.Load().([]string)
	for _, name := range []string{script.read, script.control} {
		if count := len(slices.DeleteFunc(slices.Clone(offered), func(candidate string) bool { return candidate != name })); count != 1 {
			t.Fatalf("the helper tool %s was offered %d times: %v", name, count, offered)
		}
	}
	if approvals != 1 || script.step.Load() != 4 {
		t.Fatalf("approvals %d, model steps %d", approvals, script.step.Load())
	}
	if calls := helperCalls(t, helper, "tool "); !slices.Equal(calls, []string{"tool desktop_snapshot window-1", "tool desktop_click button-7"}) {
		t.Fatalf("tool calls after the turn: %v", calls)
	}
	if runs := helperCalls(t, helper, "run mcp"); len(runs) == 0 {
		t.Fatalf("the helper never ran as an MCP server: %v", helperCalls(t, helper, "run "))
	}
	var state store.AgentState
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		state, err = store.LoadAgentState(ctx, db, session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	results := map[string]*canonical.ToolResponse{}
	for _, record := range state.Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if response, ok := block.Data.(*canonical.ToolResponse); ok {
					results[response.ID] = response
				}
			}
		}
	}
	for id, expected := range map[string]string{"read-call": "fixture:desktop_snapshot:window-1", "control-call": "fixture:desktop_click:button-7"} {
		if response := results[id]; response == nil || response.Status != canonical.ToolSuccess || response.Format != canonical.MCP || !strings.Contains(string(response.Result.Value), expected) {
			t.Fatalf("%s did not return through the history: %+v", id, response)
		}
	}
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the helper on PATH ran: %v", err)
	}
	// The enabled package allows the cursor overlay by default, so the launch
	// asks the helper for an overlay session. The stand-in refuses it and the
	// turn continues without one.
	sessions := helperCalls(t, helper, "run session")
	if len(sessions) == 0 {
		t.Fatalf("the turn's helper launch never asked for an overlay session: %v", helperCalls(t, helper, "run "))
	}
	t.Logf("step 5 passed: the turn was offered %s and %s, ran the read tool unasked, waited for one approval and then ran the control tool; session commands the helper refused: %v",
		script.read, script.control, sessions)
}
