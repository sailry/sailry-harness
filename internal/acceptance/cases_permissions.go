package acceptance

import (
	"bytes"
	"encoding/json"
	"fmt"
	"path"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const sandboxMarker = "SAILRY-SANDBOX-START\n"
const sandboxPrint = "printf 'SAILRY-SANDBOX-START\\n'"
const sandboxFile = "sandbox-boundary.txt"
const sandboxOriginal = "SAILRY-SANDBOX-UNCHANGED\n"
const sandboxWritten = "SAILRY-SANDBOX-WRITTEN\n"

func (run *caseRun) sandboxSession(permission protocol.Permission) error {
	output, err := run.command(run.ctx, "inspect_host", nil)
	if err != nil {
		return err
	}
	host, err := outputAs[protocol.HostInfo](output, "host_info")
	if err != nil {
		return err
	}
	// HostInfo exposes the platform name (for example ubuntu) when available.
	if host.Os == nil || !slices.Contains([]string{"darwin", "macos", "linux", "ubuntu", "debian", "fedora", "centos", "rhel", "alpine", "arch", "archlinux", "opensuse", "nixos"}, strings.ToLower(*host.Os)) {
		return &caseIssue{"skipped", "Native sandbox acceptance requires a known macOS or Linux execution Node"}
	}
	output, err = run.command(run.ctx, "read_sandbox_settings", nil)
	if err != nil {
		return err
	}
	settings, err := outputAs[protocol.SandboxSettings](output, "sandbox_settings")
	if err != nil {
		return err
	}
	if !settings.Enabled {
		return blocked("Sandbox acceptance requires the existing sandbox setting to be enabled; no setting was changed")
	}
	return run.newSession(protocol.WorkModeCode, permission)
}

func (run *caseRun) sandboxRoot() (string, error) {
	output, err := run.command(run.ctx, "read_worktree_catalog", protocol.CommandReadWorktreeCatalogPayload{Worktree: run.session.Worktree})
	if err != nil {
		return "", err
	}
	catalog, err := outputAs[protocol.ProjectsCatalog](output, "project_catalog")
	if err != nil {
		return "", err
	}
	for _, tree := range catalog.Worktrees {
		if tree.ID == run.session.Worktree && tree.Project == nil && path.IsAbs(tree.Path) && path.Clean(tree.Path) != "/" {
			return path.Clean(tree.Path), nil
		}
	}
	return "", blocked("Sandbox fixture has no isolated owned workspace path")
}

func (run *caseRun) sandboxFrame(recorder *Recorder, started time.Time, pending *protocol.ConversationApproval) error {
	state, known := run.controller.Conversation(run.session.ID)
	if !known || !state.Connected || state.Err != nil || state.Snapshot.Page.Session != run.session.ID {
		return fail("Sandbox recording has no connected owned conversation snapshot")
	}
	if pending != nil && !slices.ContainsFunc(state.Snapshot.Page.Approvals, func(value protocol.ConversationApproval) bool {
		return value.ID == pending.ID && value.Session == pending.Session && value.Turn == pending.Turn && value.Entry == pending.Entry && value.Index == pending.Index && value.State == protocol.ConversationApprovalStatePending && value.Access != nil && *value.Access == protocol.ProcessAccessUnrestricted && !value.CanRemember
	}) {
		return fail("Sandbox recording did not retain the exact pending unrestricted approval")
	}
	if err := recorder.Append(RecordingFrame{At: time.Since(started), Snapshot: state.Snapshot}); err != nil {
		return err
	}
	return recorder.Flush()
}

func sandboxPrompt(command, access string) string {
	argument := fmt.Sprintf("access=%q", access)
	if access == "" {
		argument = "omit the access argument entirely (this omission overrides the generic workspace-access instruction for this case)"
	}
	return fmt.Sprintf("Call the already listed run_command exactly once with command=%q, %s, background=false, timeout_ms=5000, and default cwd. Do not call other tools, retry, repair or change the command. Wait for any approval, then briefly report the actual result.", command, argument)
}

func caseSandboxAsk(run *caseRun) error {
	// A second ledger-owned workspace supplies the only out-of-sandbox path.
	// It contains no user data and is removed by the normal acceptance cleanup.
	if err := run.sandboxSession(protocol.PermissionAsk); err != nil {
		return err
	}
	outside := run.session
	if err := run.seed(sandboxFile, sandboxOriginal); err != nil {
		return err
	}
	outsideRoot, err := run.sandboxRoot()
	if err != nil {
		return err
	}
	if err := run.newSession(protocol.WorkModeCode, protocol.PermissionAsk); err != nil {
		return err
	}
	root, err := run.sandboxRoot()
	if err != nil {
		return err
	}
	if root == outsideRoot || strings.HasPrefix(outsideRoot, root+"/") || strings.HasPrefix(root, outsideRoot+"/") {
		return blocked("Sandbox fixture workspaces overlap")
	}
	recorder, err := NewRecorder(run.directory, run.controller.Target(), run.session.ID)
	if err != nil {
		return err
	}
	started := time.Now()
	view, err := run.ask(sandboxPrompt(sandboxPrint, "workspace"))
	if err != nil {
		return err
	}
	if err := run.assertSandboxCommand(view, sandboxPrint, "workspace", protocol.ProcessAccessWorkspace, protocol.ConversationApprovalSourceSandbox, true); err != nil {
		return err
	}
	// Quote the Node's POSIX path as shell data, including embedded apostrophes.
	quoted := "'" + strings.ReplaceAll(path.Join(outsideRoot, sandboxFile), "'", "'\"'\"'") + "'"
	command := sandboxPrint + "; printf 'SAILRY-SANDBOX-WRITTEN\\n' > " + quoted
	exception := "For this sandbox acceptance case only, the exact output path supplied below is a second temporary workspace owned by this same test. It is the only exception to the normal workspace-only scope; do not access any other path or session. "
	view, err = run.ask(exception + sandboxPrompt(command, "workspace"))
	if err != nil {
		return err
	}
	if err := run.assertSandboxCommand(view, command, "workspace", protocol.ProcessAccessWorkspace, protocol.ConversationApprovalSourceSandbox, false); err != nil {
		return err
	}
	if err := run.sandboxOutsideEquals(outside, sandboxOriginal); err != nil {
		return err
	}
	turn, err := run.submit(exception + "For this one retry, request unrestricted access explicitly and wait for approval. " + sandboxPrompt(command, "unrestricted"))
	if err != nil {
		return err
	}
	view, err = run.pending(turn.ID)
	if err != nil {
		return err
	}
	if len(view.approvals()) != 1 || len(view.questions()) != 0 || len(view.tools()) != 1 {
		return blocked("The unrestricted retry did not produce exactly one expected approval; no answer was sent")
	}
	approval, call := view.approvals()[0], view.tools()[0]
	detail, err := run.detail(call)
	if err != nil {
		return err
	}
	if !sandboxApprovalMatches(approval, run.session.ID, turn.ID, call, detail, command) {
		return blocked("Unrestricted approval does not match the exact owned fixture command; no answer was sent")
	}
	if err := run.assertCapture(turn); err != nil {
		return err
	}
	if err := run.sandboxOutsideEquals(outside, sandboxOriginal); err != nil {
		return err
	}
	if err := run.sandboxFrame(recorder, started, &approval); err != nil {
		return err
	}
	if _, err := run.command(run.ctx, "resolve_approval", protocol.CommandResolveApprovalPayload{Session: run.session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}); err != nil {
		return err
	}
	if run.resolved == nil {
		run.resolved = map[protocol.ApprovalID]bool{}
	}
	run.resolved[approval.ID] = true
	view, err = run.wait(turn.ID, nil)
	if err != nil {
		return err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		return fail("Unrestricted fixture did not complete after its one-time approval")
	}
	if err := run.assertSandboxCommand(view, command, "unrestricted", protocol.ProcessAccessUnrestricted, protocol.ConversationApprovalSourceUser, true); err != nil {
		return err
	}
	if err := run.sandboxOutsideEquals(outside, sandboxWritten); err != nil {
		return err
	}
	if err := run.sandboxFrame(recorder, started, nil); err != nil {
		return err
	}
	run.evidence("sandbox", "Native workspace command succeeded; exact outside fixture write was denied, then succeeded only after its non-remembered unrestricted approval")
	return CaptureRecording(run.ctx, run.controller, run.directory, run.session.ID)
}

func (run *caseRun) sandboxOutsideEquals(outside protocol.Session, expected string) error {
	if !slices.ContainsFunc(run.owned, func(value ownedSession) bool { return value.id == outside.ID }) || outside.ID == run.session.ID {
		return fail("Outside fixture is not a distinct test-owned session")
	}
	output, err := run.command(run.ctx, "read_file", protocol.CommandReadFilePayload{Worktree: outside.Worktree, Path: sandboxFile})
	if err != nil {
		return err
	}
	file, err := outputAs[protocol.FileContent](output, "file_content")
	if err != nil {
		return err
	}
	if file.Truncated || file.Text != expected {
		return fail("Sandbox boundary fixture has unexpected content")
	}
	return nil
}

func caseSandboxFull(run *caseRun) error {
	if err := run.sandboxSession(protocol.PermissionAsk); err != nil {
		return err
	}
	// Prove that isolation can start on this Node before claiming Full bypasses
	// it. An enabled setting alone does not establish a working OS sandbox.
	baseline, err := run.ask(sandboxPrompt(sandboxPrint, "workspace"))
	if err != nil {
		return err
	}
	if err := run.assertSandboxCommand(baseline, sandboxPrint, "workspace", protocol.ProcessAccessWorkspace, protocol.ConversationApprovalSourceSandbox, true); err != nil {
		return err
	}
	if err := run.newSession(protocol.WorkModeCode, protocol.PermissionFull); err != nil {
		return err
	}
	recorder, err := NewRecorder(run.directory, run.controller.Target(), run.session.ID)
	if err != nil {
		return err
	}
	started := time.Now()
	for _, access := range []string{"", "workspace"} {
		view, err := run.ask(sandboxPrompt(sandboxPrint, access))
		if err != nil {
			return err
		}
		if err := run.assertSandboxCommand(view, sandboxPrint, access, protocol.ProcessAccessUnrestricted, protocol.ConversationApprovalSourceFull, true); err != nil {
			return err
		}
		if err := run.sandboxFrame(recorder, started, nil); err != nil {
			return err
		}
	}
	run.evidence("sandbox", "Full permission executed both omitted-access and explicit-workspace requests as unrestricted processes without pending approvals")
	return CaptureRecording(run.ctx, run.controller, run.directory, run.session.ID)
}

func sandboxCommandMatches(detail protocol.ToolDetail, command, access string) bool {
	var args struct {
		Command     string            `json:"command"`
		Access      *string           `json:"access"`
		Description *string           `json:"description"`
		Cwd         *string           `json:"cwd"`
		Timeout     uint64            `json:"timeout_ms"`
		Background  bool              `json:"background"`
		FollowUp    bool              `json:"follow_up"`
		Pty         bool              `json:"pty"`
		Attachments []json.RawMessage `json:"attachments"`
	}
	decoder := json.NewDecoder(bytes.NewReader(detail.Arguments))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&args) != nil || args.Command != command || args.Timeout != 5000 || args.Background || args.FollowUp || args.Pty || len(args.Attachments) != 0 || args.Cwd != nil && *args.Cwd != "" && *args.Cwd != "." {
		return false
	}
	if access == "" {
		_, supplied := arguments(detail)["access"]
		return !supplied
	}
	return args.Access != nil && *args.Access == access
}

func sandboxApprovalMatches(approval protocol.ConversationApproval, session protocol.SessionID, turn protocol.TurnID, call toolCall, detail protocol.ToolDetail, command string) bool {
	return approval.ID != (protocol.ApprovalID{}) && approval.Session == session && approval.Turn == turn && approval.Entry != "" &&
		approval.State == protocol.ConversationApprovalStatePending && approval.Source == protocol.ConversationApprovalSourceUser &&
		approval.Access != nil && *approval.Access == protocol.ProcessAccessUnrestricted && !approval.CanRemember && approval.Prefix == nil && approval.Choice == nil &&
		approval.Startup == nil && approval.Ssh == nil && approval.Changes == nil &&
		call.name == "run_command" && call.reference.Session == session && call.reference.Turn == turn && call.reference.Entry == approval.Entry && call.reference.Index == approval.Index &&
		sandboxCommandMatches(detail, command, "unrestricted")
}

func (run *caseRun) assertSandboxCommand(view turnView, command, requested string, actual protocol.ProcessAccess, source protocol.ConversationApprovalSource, success bool) error {
	capture, err := run.capture(view.run.Turn)
	if err != nil {
		return err
	}
	permission := protocol.PermissionAsk
	if source == protocol.ConversationApprovalSourceFull {
		permission = protocol.PermissionFull
	}
	if capture.Config.Permission != permission || capture.Config.Mode != protocol.WorkModeCode {
		return fail("Sandbox turn did not capture the case's permission and mode")
	}
	calls := view.tools()
	if len(calls) != 1 || calls[0].name != "run_command" {
		return fail("Sandbox fixture did not execute exactly its one declared command")
	}
	call := calls[0]
	detail, err := run.detail(call)
	if err != nil {
		return err
	}
	if !sandboxCommandMatches(detail, command, requested) {
		return fail("Sandbox tool arguments differ from the exact fixture command")
	}
	if call.summary.Failure != nil && call.summary.Failure.Origin == protocol.ToolFailureOriginRuntime {
		return blocked("The execution Node could not start the native sandbox fixture")
	}
	if success && call.summary.Failure != nil || !success && (call.summary.Failure == nil || call.summary.Failure.Origin != protocol.ToolFailureOriginTool) {
		return fail("Sandbox command did not retain its expected native completion")
	}
	var raw struct {
		Kind string                     `json:"kind"`
		Data protocol.ProcessCompletion `json:"data"`
	}
	if detail.Result == nil || json.Unmarshal(*detail.Result, &raw) != nil || raw.Kind != "command_result" {
		return fail("Sandbox command has no canonical raw process completion")
	}
	code, err := decode[int32](raw.Data.Outcome.Data)
	if err == nil && raw.Data.Outcome.Kind == "exited" && code != 0 && raw.Data.Stdout.Text == "" && actual == protocol.ProcessAccessWorkspace {
		return blocked("The native sandbox could not start the fixture shell on this Node")
	}
	if err != nil || raw.Data.Outcome.Kind != "exited" || (code == 0) != success || raw.Data.Stdout.Text != sandboxMarker || !success && raw.Data.Stderr.Text == "" {
		return fail("Sandbox command did not prove the exact fixture success or denial")
	}
	id, err := retainedCommandID(detail)
	if err != nil {
		return err
	}
	process, err := run.processOutput(id, nil)
	if err != nil {
		return err
	}
	if process.Info.ID != id || process.Info.Session != run.session.ID || process.Info.Turn != view.run.Turn || process.Info.Command != command || process.Info.Access != actual || process.Info.Status.Kind != "finished" {
		return fail("Executed process scope or access differs from its sandbox approval")
	}
	approvals := []protocol.ConversationApproval{}
	for _, approval := range view.page.Approvals {
		if approval.Turn == view.run.Turn {
			approvals = append(approvals, approval)
		}
	}
	if len(approvals) != 1 || approvals[0].Session != run.session.ID || approvals[0].State != protocol.ConversationApprovalStateApproved || approvals[0].Source != source || approvals[0].Access == nil || *approvals[0].Access != actual {
		return fail("Canonical approval does not match the executed permission and access")
	}
	if source == protocol.ConversationApprovalSourceUser && (approvals[0].CanRemember || approvals[0].Prefix != nil || approvals[0].Choice == nil || approvals[0].Choice.Kind != "once") {
		return fail("Unrestricted retry was not approved only once")
	}
	run.evidence("process", fmt.Sprintf("turn=%s command=%s requested=%q actual=%s exit=%d source=%s", view.run.Turn, id, requested, actual, code, source))
	return nil
}
