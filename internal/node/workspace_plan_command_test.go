package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReadOnlyChildCommandLaunch(t *testing.T) {
	t.Parallel()
	launches := make(chan process.Launch, 4)
	f := newStopFixture(t, func(_ context.Context, launch process.Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		launches <- launch
		emit(protocol.ProcessOutputOutputStreamStdout, []byte("child inspected the workspace\n"))
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	})
	config := f.session.Config
	config.Mode = protocol.WorkModeFlagship
	f.session = commandFixture(t, f.node, protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{
		Session: f.session.ID, ExpectedRevision: f.session.Revision, Config: config,
	}}).Data.(protocol.Session)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.ReadOnly = protocol.NewRoleID(), "plan-reader", "Plan reader", true
	role = commandFixture(t, f.node, protocol.Command{Kind: "put_role", Data: protocol.CommandPutRolePayload{Role: role}}).Data.(protocol.RoleProfile)
	f.session = commandFixture(t, f.node, protocol.Command{Kind: "set_session_roles", Data: protocol.CommandSetSessionRolesPayload{
		Session: f.session.ID, ExpectedRevision: f.session.Revision, Roles: []protocol.RoleReference{{ID: role.ID, Revision: role.Revision}},
	}}).Data.(protocol.Session)
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		switch step {
		case 1:
			return [][3]string{{"child", "spawn_agent", `{"task":"Inspect the workspace","role":"plan-reader","overlap":false}`}}
		case 2:
			return [][3]string{
				{"child-run", "run_command", `{"command":"printf '%s\\n' \"$(pwd)\""}`},
				{"child-escape", "run_command", `{"command":"pwd","access":"unrestricted"}`},
			}
		}
		return nil
	})
	f.submit("Delegate read-only inspection to the selected role")
	f.until("the parent turn to complete", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCompleted
	})
	var child protocol.ConversationHistory
	if err := f.node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		parent, err := store.ReadConversation(t.Context(), db, f.session.ID, nil, 100)
		if err != nil {
			return err
		}
		if len(parent.Page.Children) != 1 || parent.Page.Children[0].Run.Status != protocol.ConversationStatusCompleted {
			t.Errorf("unexpected delegated runs: %+v", parent.Page.Children)
			return nil
		}
		child, err = store.ReadConversation(t.Context(), db, parent.Page.Children[0].Run.Session, nil, 100)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if len(child.Page.Runs) != 1 {
		t.Fatalf("expected one child turn: %+v", child.Page.Runs)
	}
	f.script.mu.Lock()
	defer f.script.mu.Unlock()
	if escape := f.script.results["child-escape"]; !strings.Contains(escape, "permission_denied") || !strings.Contains(escape, "unrestricted access is not allowed") {
		t.Fatalf("read-only child unrestricted call = %q", escape)
	}
	result, approvals := f.script.results["child-run"], child.Page.Approvals
	if !process.SandboxSupported() {
		if len(launches) != 0 || len(approvals) != 0 || !strings.Contains(result, "permission_denied") {
			t.Fatalf("unsupported sandbox allowed child command: %q, launches=%d, approvals=%+v", result, len(launches), approvals)
		}
		return
	}
	if len(launches) != 1 || !strings.Contains(result, "child inspected the workspace") || !strings.Contains(result, `"kind":"command_result"`) {
		t.Fatalf("child command did not complete in the foreground: %q, launches=%d", result, len(launches))
	}
	launch := <-launches
	if launch.Turn != child.Page.Runs[0].Turn || !launch.ReadOnly || launch.Access != protocol.ProcessAccessWorkspace || launch.Background || launch.Pty || launch.Service {
		t.Fatalf("read-only child inherited unrestricted launch: %+v", launch)
	}
	if len(approvals) != 1 || approvals[0].State != protocol.ConversationApprovalStateApproved || approvals[0].Source != protocol.ConversationApprovalSourceSandbox {
		t.Fatalf("unexpected read-only child command approvals: %+v", approvals)
	}
}

func TestPlanCommandCannotLaunchAfterSandboxDisabled(t *testing.T) {
	t.Parallel()
	if !process.SandboxSupported() {
		t.Skip("requires an OS that can grant a sandbox approval")
	}
	var launches atomic.Int32
	f := newStopFixture(t, func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
		launches.Add(1)
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	})
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	started, finish := make(chan struct{}), make(chan struct{})
	defer close(finish)
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		if step == 1 {
			close(started)
			select {
			case <-finish:
			case <-ctx.Done():
			}
		}
		return nil
	})
	config := f.session.Config
	config.Mode = protocol.WorkModePlan
	f.session = commandFixture(t, f.node, protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{
		Session: f.session.ID, ExpectedRevision: f.session.Revision, Config: config,
	}}).Data.(protocol.Session)
	f.submit("Inspect the workspace")
	f.wait("the planning turn's first model request", started)
	runs, _, _ := f.read()
	if len(runs) != 1 || runs[0].Status != protocol.ConversationStatusRunning {
		t.Fatalf("expected a running Plan turn: %+v", runs)
	}
	run := runs[0]
	root, err := f.node.workspaceRoot(ctx, run.Worktree, false)
	if err != nil {
		t.Fatal(err)
	}
	inv := &store.AgentInvocation{Caller: f.node.ID(), Run: run, Session: f.session, Turn: protocol.QueuedTurn{ID: run.Turn, Config: config}, Worktree: protocol.Worktree{ID: run.Worktree, Project: f.session.Project, Path: root}}
	host := nodeModuleHost{node: f.node}
	tool, err := host.WorkspaceTool(inv, moduleapi.WorkspaceToolConfig{Spec: moduleapi.ToolSpec{
		Name: "module_run", Operation: protocol.ToolOperationRunCommand, Parameters: json.RawMessage(`{"type":"object","properties":{"command":{"type":"string"}}}`),
	}})
	if err != nil {
		t.Fatal(err)
	}
	call := canonical.ToolRequest{ID: "module-command", Name: "module_run", Arguments: json.RawMessage(`{"command":"pwd"}`)}
	if err := tool.Prepare(ctx, call); err != nil {
		t.Fatal(err)
	}
	defer tool.ReleasePrepared(call)
	var approval protocol.ConversationApproval
	var requestID protocol.RequestID
	if err := f.node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		message := canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &call})
		record := canonical.NewRecord(run.Turn, "message", &message, time.Now())
		record.Provenance.Author = "sailry"
		if _, _, err := store.AppendCanonical(ctx, db, run.Session, record); err != nil {
			return err
		}
		var err error
		approval, _, err = store.BeginApproval(ctx, db, run.Turn, canonical.ToolIdentity{ID: &call.ID, Name: call.Name, Arguments: call.Arguments}, tool.Binding, time.Now())
		if err != nil {
			return err
		}
		approval, err = tool.ConfigureApproval(ctx, db, call, approval)
		if err != nil {
			return err
		}
		var id string
		if err := db.QueryRowContext(ctx, "UPDATE agent_approvals SET claimed=1 WHERE id=?1 RETURNING request", approval.ID.String()).Scan(&id); err != nil {
			return err
		}
		requestID, err = protocol.ParseRequestID(id)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if approval.State != protocol.ConversationApprovalStateApproved || approval.Source != protocol.ConversationApprovalSourceSandbox || approval.Access == nil || *approval.Access != protocol.ProcessAccessWorkspace {
		t.Fatalf("module command approval did not capture workspace authority: %+v", approval)
	}
	tool.Approved(call, approval.ID, requestID)
	setCommandSandbox(t, f.node, false)
	if result := tool.Execute(ctx, call); result.Failure == nil || result.Failure.Code != protocol.ErrorCodePermissionDenied || !strings.Contains(result.Failure.Message, "enabled read-only OS sandbox") {
		t.Fatalf("previously approved command was not refused at launch: %+v", result)
	}
	input := protocol.DefaultCommandRunCommandPayload()
	input.Turn, input.Command, input.Access = run.Turn, "pwd", protocol.ProcessAccessWorkspace
	input.TimeoutMs = 5000
	request := protocol.NewRequest(f.node.ID(), protocol.Command{Kind: "run_command", Data: &input})
	if result := dispatchFixture(t, f.node, f.node.ID(), request); result.Fault == nil || result.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("direct command request ignored the disabled sandbox: %+v", result)
	}
	if count := launches.Load(); count != 0 {
		t.Fatalf("disabled Plan sandbox allowed %d launches", count)
	}
}
