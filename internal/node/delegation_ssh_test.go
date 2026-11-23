package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type delegatedSSHModel struct {
	fantasy.LanguageModel
	step int
	next func(context.Context, int, fantasy.Call) ([]fantasy.StreamPart, error)
}

func (m *delegatedSSHModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	m.step++
	parts, err := m.next(ctx, m.step, call)
	if err != nil {
		return nil, err
	}
	return func(yield func(fantasy.StreamPart) bool) {
		for _, part := range parts {
			if !yield(part) {
				return
			}
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

func TestChildInheritsCapturedSSHTools(t *testing.T) {
	t.Parallel()
	peer := newSSHLogPeer(t)
	peer.allowPassword.Store(true)
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	parentReady, spawn := make(chan struct{}), make(chan struct{})
	var parent protocol.Session
	var shared, late protocol.SshProfile
	tool := func(id, name string, args any) fantasy.StreamPart {
		arguments, err := json.Marshal(args)
		if err != nil {
			panic(err)
		}
		return fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: id, ToolCallName: name, ToolCallInput: string(arguments)}
	}
	answer := func(text string) []fantasy.StreamPart {
		return []fantasy.StreamPart{{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: text}}
	}
	resultText := func(call fantasy.Call, id string) string {
		for _, message := range call.Prompt {
			for _, part := range message.Content {
				result, ok := fantasy.AsMessagePart[fantasy.ToolResultPart](part)
				if !ok || result.ToolCallID != id {
					continue
				}
				switch output := result.Output.(type) {
				case fantasy.ToolResultOutputContentText:
					return output.Text
				case fantasy.ToolResultOutputContentError:
					return output.Error.Error()
				}
			}
		}
		return ""
	}
	parentModel := &delegatedSSHModel{next: func(ctx context.Context, step int, _ fantasy.Call) ([]fantasy.StreamPart, error) {
		if step == 1 {
			close(parentReady)
			select {
			case <-spawn:
			case <-ctx.Done():
				return nil, ctx.Err()
			}
			return []fantasy.StreamPart{tool("delegate", "spawn_agent", map[string]any{"task": "Check the fixture SSH connection", "overlap": false})}, nil
		}
		return answer("Child checked the connection"), nil
	}}
	childModel := &delegatedSSHModel{next: func(_ context.Context, step int, call fantasy.Call) ([]fantasy.StreamPart, error) {
		invoke := func(id, name string, args any) fantasy.StreamPart {
			for _, offered := range call.Tools {
				if offered.GetName() == name {
					return tool(id, name, args)
				}
			}
			return tool(id, "call_tool", map[string]any{"name": name, "arguments": args})
		}
		empty := map[string]any{}
		sshTools := []string{"ssh_connections", "ssh_save_connection", "ssh_run", "ssh_transfer", "database_connections"}
		scheduling := []string{"spawn_agent", "guide_agent", "wait_agent", "stop_agent", "list_subagents"}
		switch step {
		case 1:
			return []fantasy.StreamPart{tool("discover", "tool_search", map[string]any{"query": "select:" + strings.Join(append(sshTools, scheduling...), ","), "limit": 8})}, nil
		case 2:
			results := resultText(call, "discover")
			for _, name := range sshTools {
				if !strings.Contains(results, `"`+name+`"`) {
					return nil, fmt.Errorf("inherited tool %s not discovered: %s", name, results)
				}
			}
			for _, name := range scheduling {
				if strings.Contains(results, `"`+name+`"`) {
					return nil, fmt.Errorf("scheduling tool %s was inherited", name)
				}
			}
			return []fantasy.StreamPart{invoke("ssh-list", "ssh_connections", empty), invoke("database-list", "database_connections", empty),
				invoke("late", "ssh_run", map[string]any{"connection": late.ID, "command": sshLogPlatform})}, nil
		case 3:
			if listed := resultText(call, "ssh-list"); !strings.Contains(listed, shared.ID.String()) || strings.Contains(listed, late.ID.String()) {
				return nil, fmt.Errorf("child SSH catalog exceeds parent capture: %s", listed)
			}
			if listed := resultText(call, "database-list"); !strings.Contains(listed, "database_profiles") {
				return nil, fmt.Errorf("inherited database tool failed: %s", listed)
			}
			if rejected := resultText(call, "late"); !strings.Contains(rejected, "unavailable to this session") {
				return nil, fmt.Errorf("late SSH target was not rejected: %s", rejected)
			}
			return []fantasy.StreamPart{invoke("run", "ssh_run", map[string]any{"connection": shared.ID, "command": sshLogPlatform})}, nil
		case 4:
			if result := resultText(call, "run"); !strings.Contains(result, "x86_64") {
				return nil, fmt.Errorf("inherited SSH run failed: %s", result)
			}
			return []fantasy.StreamPart{invoke("transfer", "ssh_transfer", map[string]any{"connection": shared.ID, "path": "result.txt", "remote_path": "/tmp/fixture-result", "direction": "upload"}),
				invoke("save", "ssh_save_connection", map[string]any{"connection": shared.ID, "expected_revision": shared.Revision + 1, "name": "Renamed fixture"})}, nil
		case 5:
			if result := resultText(call, "transfer"); !strings.Contains(result, "denied") {
				return nil, fmt.Errorf("SSH transfer did not retain its approval: %s", result)
			}
			if result := resultText(call, "save"); !strings.Contains(result, "Renamed fixture") {
				return nil, fmt.Errorf("approved SSH save failed: %s", result)
			}
			return answer("Checked captured SSH tools"), nil
		default:
			return nil, fmt.Errorf("unexpected child request %d", step)
		}
	}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{Assembly: hostassembly.Harness(), MCP: fixtureMCPOptions(), Environment: &environment,
		Model: func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
			if binding.Session == parent.ID {
				return parentModel, nil
			}
			return childModel, nil
		}})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "SSH delegation", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance()}}).Data.(protocol.Project)
	save := func(name string, sharing protocol.ConnectionSharing) protocol.SshProfile {
		t.Helper()
		return commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
			Profile: protocol.SshProfile{ID: protocol.NewSshID(), Name: name, Host: peer.address.IP.String(), Port: uint16(peer.address.Port),
				Username: sshLogUser, Authentication: protocol.SshAuthenticationPassword, Sharing: &sharing},
			Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(sshLogPassword)}},
		}}).Data.(protocol.SshProfile)
	}
	shared = save("Project fixture", protocol.ConnectionSharing{Kind: "projects", Data: &[]protocol.ProjectID{project.ID}})
	if err = os.WriteFile(filepath.Join(root, "result.txt"), []byte("fixture\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	config := connectionModelConfig(t, node)
	config.Mode, config.Permission = protocol.WorkModeFlagship, protocol.PermissionProject
	parent = commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	changes, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer changes.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: parent.ID, ExpectedRevision: parent.Revision, Message: protocol.ConversationInput{Text: "Delegate the SSH fixture check"}}})
	select {
	case <-parentReady:
	case <-ctx.Done():
		t.Fatal("parent did not reach its first model request")
	}
	late = save("Late fixture", protocol.ConnectionSharing{Kind: "global"})
	close(spawn)
	seen := map[protocol.ApprovalID]bool{}
	var trust, transfer, saved bool
	for {
		var session protocol.Session
		var run protocol.ConversationRun
		var approvals []protocol.ConversationApproval
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			history, err := store.ReadConversation(ctx, db, parent.ID, nil, 100)
			if err != nil || len(history.Page.Children) == 0 {
				return err
			}
			run = history.Page.Children[0].Run
			session, err = store.ReadSession(ctx, db, run.Session)
			if err != nil {
				return err
			}
			child, err := store.ReadConversation(ctx, db, run.Session, nil, 100)
			approvals = child.Page.Approvals
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		if run.Status == protocol.ConversationStatusFailed || run.Status == protocol.ConversationStatusCancelled || run.Status == protocol.ConversationStatusInterrupted {
			t.Fatalf("SSH child ended %s: %v", run.Status, run.Error)
		}
		if run.Status == protocol.ConversationStatusCompleted {
			if session.Project == nil || *session.Project != project.ID || session.Config.Permission != config.Permission || !trust || !transfer || !saved || !peer.passwordSeen.Load() {
				t.Fatalf("child scope or effects differ: permission=%s approvals=%v/%v/%v", session.Config.Permission, trust, transfer, saved)
			}
			return
		}
		for _, approval := range approvals {
			if approval.State != protocol.ConversationApprovalStatePending || seen[approval.ID] {
				continue
			}
			var operation protocol.ToolOperation
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var binding []byte
				if err := db.QueryRowContext(ctx, "SELECT binding FROM agent_approvals WHERE id=?1", approval.ID.String()).Scan(&binding); err != nil {
					return err
				}
				var captured store.ApprovalBinding
				if err := json.Unmarshal(binding, &captured); err != nil {
					return err
				}
				if captured.Operation == nil {
					return fmt.Errorf("SSH approval has no operation binding: %s", captured.Tool)
				}
				operation = *captured.Operation
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			decision := protocol.ConversationDecision{Kind: "approve"}
			switch operation {
			case protocol.ToolOperationRunSsh:
				if approval.Session != run.Session || approval.Source != protocol.ConversationApprovalSourceUser || approval.Ssh == nil || approval.Ssh.Profile != shared.ID || approval.Ssh.Key != peer.key || approval.CanRemember || peer.passwordSeen.Load() {
					t.Fatalf("unexpected child fingerprint approval: %+v", approval)
				}
				trust = true
			case protocol.ToolOperationTransferSsh:
				decision.Kind = "deny"
				transfer = true
			case protocol.ToolOperationSaveSshConnection:
				saved = true
			default:
				t.Fatalf("unexpected child approval for %s", operation)
			}
			commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: approval.Session, Approval: approval.ID, Decision: decision}})
			seen[approval.ID] = true
		}
		if _, err := changes.Next(ctx); err != nil {
			t.Fatal(err)
		}
	}
}
