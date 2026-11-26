package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ordinarySessionCommandFixture(t *testing.T, script *agentToolScript, options process.Options) (*Node, protocol.SessionConfig) {
	t.Helper()
	environment := process.NewEnvironment(nil)
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment,
		MCP:         fixtureMCPOptions(),
		Model:       func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process:     options,
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.Name = protocol.NewProviderID(), true, "Command fixture"
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "https://fixture.invalid/v1"
	provider.DefaultModel = "fixture"
	entry := protocol.DefaultConversationModel()
	entry.ID, entry.Context, entry.Output, entry.Tools = provider.DefaultModel, 64000, 8192, true
	provider.Models = []protocol.ConversationModel{entry}
	secret := protocol.NewSecret("command-fixture-key")
	provider = commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}}).Data.(protocol.ConversationProvider)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
	return node, config
}

func setCommandSandbox(t *testing.T, node *Node, enabled bool) {
	t.Helper()
	current := commandFixture(t, node, protocol.Command{Kind: "read_sandbox_settings"}).Data.(protocol.SandboxSettings)
	current.Enabled = enabled
	commandFixture(t, node, protocol.Command{Kind: "save_sandbox_settings", Data: &current})
}

func ordinarySessionStarted(t *testing.T, ctx context.Context, subscription link.Subscription, turn protocol.QueuedTurn) protocol.Session {
	t.Helper()
	for {
		update, err := subscription.Next(ctx)
		if err != nil {
			t.Fatalf("task_started for %s: %v", turn.ID, err)
		}
		if event, ok := update.Data.(protocol.EventEnvelope); ok {
			if started, ok := event.Event.Data.(*protocol.EventTaskStartedPayload); ok && started.Turn.ID == turn.ID {
				session := started.Session
				if session == nil || session.ID != turn.Session || session.Project != nil || session.Config.Resource != nil || session.Config.Assistant != nil || session.Worktree == (protocol.WorktreeID{}) {
					t.Fatalf("expected a new ordinary session with a private worktree, got %+v", session)
				}
				if session.Config.Mode != turn.Config.Mode || session.Config.Permission != turn.Config.Permission {
					t.Fatalf("session configuration = %+v, admitted configuration = %+v", session.Config, turn.Config)
				}
				return *session
			}
		}
	}
}

type ordinaryCommandReceipt struct {
	Kind    string                     `json:"kind"`
	Data    protocol.ProcessCompletion `json:"data"`
	Command struct {
		ID protocol.RequestID `json:"id"`
	} `json:"sailry_command"`
}

func ordinaryCommandReadCall(callID, result string) ([3]string, bool) {
	var receipt ordinaryCommandReceipt
	if err := json.Unmarshal([]byte(result), &receipt); err != nil || receipt.Kind != "command_result" || receipt.Command.ID == (protocol.RequestID{}) {
		return [3]string{}, false
	}
	return [3]string{callID, "read_command", `{"id":"` + receipt.Command.ID.String() + `","stream":"stdout"}`}, true
}

func expectOrdinaryCommandReadback(t *testing.T, launchResult, readResult, stdout string) protocol.RequestID {
	t.Helper()
	var launched ordinaryCommandReceipt
	if err := json.Unmarshal([]byte(launchResult), &launched); err != nil {
		t.Fatalf("cannot decode run_command receipt %q: %v", launchResult, err)
	}
	code, _ := launched.Data.Outcome.Data.(*int32)
	if launched.Kind != "command_result" || launched.Command.ID == (protocol.RequestID{}) || launched.Data.Outcome.Kind != "exited" || code == nil || *code != 0 || launched.Data.Stdout.Text != stdout {
		t.Fatalf("expected command success with stdout %q, got receipt %q", stdout, launchResult)
	}
	var page struct {
		Kind string `json:"kind"`
		Data struct {
			Info struct {
				ID protocol.RequestID `json:"id"`
			} `json:"info"`
			Stream protocol.ProcessOutputOutputStream `json:"stream"`
			Text   string                             `json:"text"`
		} `json:"data"`
	}
	if err := json.Unmarshal([]byte(readResult), &page); err != nil || page.Kind != "command_output_page" || page.Data.Info.ID != launched.Command.ID || page.Data.Stream != protocol.ProcessOutputOutputStreamStdout || page.Data.Text != stdout {
		t.Fatalf("model read_command receipt = %q (%v); run_command receipt = %q", readResult, err, launchResult)
	}
	return launched.Command.ID
}

func TestOrdinarySessionCommandRoundTrip(t *testing.T) {
	t.Parallel()
	const command = "echo command fixture"
	const stdout = "command fixture\n"
	script := &agentToolScript{}
	launches := make(chan process.Launch, 8)
	node, defaults := ordinarySessionCommandFixture(t, script, process.Options{Run: func(ctx context.Context, launch process.Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		select {
		case launches <- launch:
		case <-ctx.Done():
			return protocol.ProcessCompletion{}, ctx.Err()
		}
		emit(protocol.ProcessOutputOutputStreamStdout, []byte(stdout))
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	}})
	defaults.Mode, defaults.Permission = protocol.WorkModePlan, protocol.PermissionAsk
	worktrees := make(map[protocol.WorktreeID]bool)
	for _, test := range []struct {
		name       string
		explicit   bool
		mode       protocol.WorkMode
		permission protocol.Permission
		cwd        string
	}{
		{name: "explicit Code and Full", explicit: true, mode: protocol.WorkModeCode, permission: protocol.PermissionFull},
		{name: "explicit Plan and Full", explicit: true, mode: protocol.WorkModePlan, permission: protocol.PermissionFull},
		{name: "saved Plan and Ask", mode: protocol.WorkModePlan, permission: protocol.PermissionAsk},
		{name: "saved Code and Ask", mode: protocol.WorkModeCode, permission: protocol.PermissionAsk, cwd: "subdir"},
	} {
		if !t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
			defer cancel()
			if test.mode == protocol.WorkModePlan {
				defer setCommandSandbox(t, node, true)
			}
			saved := defaults
			if !test.explicit {
				saved.Mode, saved.Permission = test.mode, test.permission
			}
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				value, err := store.ReadDefaults(ctx, db)
				if err == nil {
					value.Config = &saved
					_, err = store.SaveDefaults(ctx, db, node.ID(), value)
				}
				return err
			}); err != nil {
				t.Fatal(err)
			}
			args := map[string]string{"command": command}
			if test.cwd != "" {
				args["cwd"] = test.cwd
			}
			wantAccess, wantSource, sandbox := protocol.ProcessAccessUnrestricted, protocol.ConversationApprovalSourceFull, "disabled"
			if test.mode == protocol.WorkModePlan || test.permission == protocol.PermissionAsk {
				if process.SandboxSupported() {
					wantAccess, wantSource, sandbox = protocol.ProcessAccessWorkspace, protocol.ConversationApprovalSourceSandbox, "enabled"
				} else if test.mode != protocol.WorkModePlan {
					args["access"], wantSource = string(protocol.ProcessAccessUnrestricted), protocol.ConversationApprovalSourceUser
				}
			}
			arguments, err := json.Marshal(args)
			if err != nil {
				t.Fatal(err)
			}
			script.reset(func(step int, results map[string]string) [][3]string {
				switch step {
				case 1:
					if test.mode == protocol.WorkModePlan {
						return [][3]string{
							{"launch", "run_command", string(arguments)},
							{"escape", "run_command", `{"command":"echo escape","access":"unrestricted"}`},
							{"background", "run_command", `{"command":"pwd","background":true}`},
							{"service", "run_command", `{"command":"pwd","background":true,"service":true}`},
							{"pty", "run_command", `{"command":"pwd","background":true,"pty":true}`},
						}
					}
					return [][3]string{{"launch", "run_command", string(arguments)}}
				case 2:
					if test.mode == protocol.WorkModePlan {
						// A new call in the same admitted turn must honor the
						// changed setting, including for a previously allowlisted command.
						setCommandSandbox(t, node, false)
						calls := [][3]string{{"disabled", "run_command", `{"command":"pwd"}`}}
						if call, ok := ordinaryCommandReadCall("read", results["launch"]); ok {
							calls = append(calls, call)
						}
						return calls
					}
					if call, ok := ordinaryCommandReadCall("read", results["launch"]); ok {
						return [][3]string{call}
					}
				}
				return nil
			})
			subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
			if err != nil {
				t.Fatal(err)
			}
			defer subscription.Close()
			start := protocol.DefaultConversationStart()
			start.Execution, start.Message = protocol.ConversationExecutionModel, protocol.ConversationInput{Text: "Run the fixture and read its output"}
			if test.explicit {
				config := defaults
				config.Mode, config.Permission = test.mode, test.permission
				start.Config, start.UseExplicitConfig = &config, true
			}
			turn := commandFixture(t, node, protocol.Command{Kind: "start_session", Data: start}).Data.(protocol.QueuedTurn)
			if turn.Config.Mode != test.mode || turn.Config.Permission != test.permission || turn.Config.Provider != defaults.Provider || turn.Config.Model != defaults.Model {
				t.Fatalf("admitted configuration = %+v", turn.Config)
			}
			session := ordinarySessionStarted(t, ctx, subscription, turn)
			if worktrees[session.Worktree] {
				t.Fatalf("ordinary sessions shared worktree %s", session.Worktree)
			}
			worktrees[session.Worktree] = true
			root := core.SessionWorkspace(node.profile.Ownership.Path, session.ID)
			var run protocol.ConversationRun
			var approvals []protocol.ConversationApproval
			for {
				if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
					var err error
					run, err = store.ReadAgentRun(ctx, db, turn.ID)
					if err != nil {
						return err
					}
					history, err := store.ReadConversation(ctx, db, session.ID, nil, 100)
					approvals = history.Page.Approvals
					return err
				}); err != nil {
					t.Fatal(err)
				}
				if run.Status == protocol.ConversationStatusCompleted {
					break
				}
				switch run.Status {
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("ordinary turn ended with %s: %v", run.Status, run.Error)
				}
				for _, approval := range approvals {
					if approval.State == protocol.ConversationApprovalStatePending {
						if wantSource != protocol.ConversationApprovalSourceUser || test.mode == protocol.WorkModePlan {
							t.Fatalf("unexpected pending approval: %+v", approval)
						}
						commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{
							Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"},
						}})
					}
				}
				if _, err := subscription.Next(ctx); err != nil {
					t.Fatalf("waiting for turn %s in state %s: %v", turn.ID, run.Status, err)
				}
			}
			if run.Session != session.ID || run.Worktree != session.Worktree || run.Kind != protocol.ConversationRunKindTask {
				t.Fatalf("run did not use the new session and worktree: %+v", run)
			}
			if info, err := os.Stat(root); err != nil || !info.IsDir() {
				t.Fatalf("session workspace %q was not created: %v", root, err)
			}
			script.mu.Lock()
			defer script.mu.Unlock()
			if len(script.offered) == 0 {
				t.Fatal("no model request for the admitted turn")
			}
			for _, tool := range []string{"run_command", "read_command", "stop_command"} {
				if !slices.Contains(script.offered[0], tool) {
					t.Errorf("initial model request omitted %s: %v", tool, script.offered[0])
				}
			}
			if want := "\nWorking directory: . (session workspace)\nCommand sandbox: " + sandbox; !strings.Contains(script.prompts[0], want) {
				t.Errorf("initial model request omitted workspace context %q", want)
			}
			launchResult := script.results["launch"]
			if test.mode == protocol.WorkModePlan {
				for _, id := range []string{"escape", "background", "service", "pty", "disabled"} {
					if result := script.results[id]; !strings.Contains(result, "permission_denied") || !strings.Contains(result, "planning mode permits only") {
						t.Errorf("Plan command %s was not refused: %q", id, result)
					}
				}
				if !process.SandboxSupported() {
					if len(launches) != 0 || len(approvals) != 0 || !strings.Contains(launchResult, "planning mode permits only") {
						t.Fatalf("Plan command without sandbox produced %d launches, %d approvals, and receipt %q", len(launches), len(approvals), launchResult)
					}
					return
				}
				if len(launches) != 1 {
					t.Fatalf("Plan sandboxed command produced %d launches and receipt %q", len(launches), launchResult)
				}
				id := expectOrdinaryCommandReadback(t, launchResult, script.results["read"], stdout)
				launch := <-launches
				if launch.ID != id || !launch.ReadOnly || launch.Access != protocol.ProcessAccessWorkspace || launch.Background || launch.Service || launch.Pty {
					t.Fatalf("Plan sandboxed launch was not confined read-only: %+v", launch)
				}
				if len(approvals) != 1 || approvals[0].State != protocol.ConversationApprovalStateApproved || approvals[0].Source != protocol.ConversationApprovalSourceSandbox || approvals[0].Access == nil || *approvals[0].Access != protocol.ProcessAccessWorkspace {
					t.Fatalf("Plan sandboxed approval = %+v", approvals)
				}
				return
			}
			if len(launches) != 1 {
				t.Fatalf("expected one successful command, got %d launches and receipt %q", len(launches), launchResult)
			}
			id := expectOrdinaryCommandReadback(t, launchResult, script.results["read"], stdout)
			launch := <-launches
			if launch.ID != id || launch.Session != session.ID || launch.Turn != turn.ID || launch.Worktree != session.Worktree || launch.Root != root || launch.Cwd != test.cwd || launch.Access != wantAccess || launch.ReadOnly || launch.Command != command || launch.Background {
				t.Fatalf("unexpected launch for %q with cwd %q and access %s: %+v", root, test.cwd, wantAccess, launch)
			}
			if len(approvals) != 1 || approvals[0].State != protocol.ConversationApprovalStateApproved || approvals[0].Source != wantSource || approvals[0].Access == nil || *approvals[0].Access != wantAccess {
				t.Fatalf("expected a %s approval for %s access, got %+v", wantSource, wantAccess, approvals)
			}
		}) {
			return
		}
	}
}
