package acceptance

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const taskScope = "This is an explicitly requested Sailry acceptance case. Work only in this conversation's dedicated workspace. Do not access credentials, environment variables, system files, network, other sessions or user repositories. Do not install anything or delegate. Use only the tools named in the task. Run a shell command only if the task supplies its exact text; use workspace access. Keep the final reply brief unless the task explicitly requests a longer streaming sample.\n\n"

type ownedSession struct {
	id protocol.SessionID
}

type caseRun struct {
	ctx         context.Context
	controller  *client.Client
	directory   string
	config      protocol.SessionConfig
	result      *CaseResult
	session     protocol.Session
	owned       []ownedSession
	turns       []protocol.QueuedTurn
	releases    []func()
	wake        chan struct{}
	answered    map[protocol.QuestionID]bool
	resolved    map[protocol.ApprovalID]bool
	unconfirmed *protocol.Request
	captures    map[protocol.TurnID]protocol.QueuedTurn
	pluginTools map[protocol.PluginReference]map[string]string
}

func (run *caseRun) evidence(kind, value string) {
	if len(run.result.Evidence) < 80 {
		run.result.Evidence = append(run.result.Evidence, Evidence{kind, bounded(value, 768)})
	}
}

func (run *caseRun) command(ctx context.Context, kind string, data any) (protocol.Output, error) {
	request := run.controller.Prepare(protocol.Command{Kind: kind, Data: data})
	if request.Command.Durable() {
		run.evidence("request", kind+" "+request.ID.String())
	}
	output, err := executeRecorded(ctx, run.controller, run.directory, request)
	if err == nil {
		return output, nil
	}
	fault, ok := nodeFault(err)
	if !ok || fault.Code != protocol.ErrorCodeOutcomeUnknown {
		return output, err
	}
	if run.unconfirmed == nil {
		run.unconfirmed = &request
	}
	return protocol.Output{}, blocked("Admission is unconfirmed; retain the recorded request ID and do not resubmit")
}

func outputAs[T any](output protocol.Output, kind string) (T, error) {
	var value T
	if output.Kind != kind {
		return value, fail("Node returned an unexpected output kind for " + kind)
	}
	value, err := decode[T](output.Data)
	if err != nil {
		return value, fail("Node returned an invalid " + kind + " payload")
	}
	return value, nil
}

func (run *caseRun) newSession(mode protocol.WorkMode, permission protocol.Permission) error {
	config := run.config
	config.Mode, config.Permission = mode, permission
	output, err := run.command(run.ctx, "create_session", protocol.CommandCreateSessionPayload{Config: &config})
	if err != nil {
		return err
	}
	session, err := outputAs[protocol.Session](output, "session")
	if err != nil {
		return err
	}
	run.session = session
	run.result.Session = session.ID.String()
	if err := run.own(session); err != nil {
		return err
	}
	if session.Project != nil || session.Config.Resource != nil || session.Config.Assistant != nil {
		return fail("Acceptance session was assigned to a non-fixture resource")
	}
	output, err = run.command(run.ctx, "set_session_team", protocol.CommandSetSessionExecutionPayload{Session: session.ID, ExpectedRevision: session.Revision, Execution: protocol.ConversationExecutionModel})
	if err != nil {
		return err
	}
	if err := run.updateSession(output); err != nil {
		return err
	}
	output, err = run.command(run.ctx, "rename_session", protocol.CommandRenameSessionPayload{Session: run.session.ID, ExpectedRevision: run.session.Revision, Title: "Acceptance " + run.result.ID})
	if err != nil {
		return err
	}
	return run.updateSession(output)
}

func (run *caseRun) own(session protocol.Session) error {
	if err := Own(run.directory, run.controller.Target(), session.ID); err != nil {
		return err
	}
	if slices.ContainsFunc(run.owned, func(value ownedSession) bool { return value.id == session.ID }) {
		return nil
	}
	run.owned = append(run.owned, ownedSession{session.ID})
	run.result.OwnedSessions = append(run.result.OwnedSessions, session.ID.String())
	changed := func() {
		select {
		case run.wake <- struct{}{}:
		default:
		}
	}
	release, err := run.controller.WatchConversation(run.ctx, session.ID, changed)
	if err != nil {
		return err
	}
	run.releases = append(run.releases, release)
	changed()
	return nil
}

func (run *caseRun) refreshSession() error {
	output, err := run.command(run.ctx, "read_session", protocol.CommandReadSessionPayload{Session: run.session.ID})
	if err != nil {
		return err
	}
	return run.updateSession(output)
}

func (run *caseRun) updateSession(output protocol.Output) error {
	session, err := outputAs[protocol.Session](output, "session")
	if err != nil {
		return err
	}
	if session.ID != run.session.ID {
		return fail("Node returned another session instead of the owned fixture")
	}
	// Keep the captured session and ownership intact if an update cannot be
	// decoded, so cleanup still targets the session this case created.
	run.session = session
	return nil
}

func message(text string) protocol.ConversationInput {
	input := protocol.DefaultConversationInput()
	input.Text = taskScope + text
	return input
}

func (run *caseRun) submit(text string) (protocol.QueuedTurn, error) {
	if err := run.refreshSession(); err != nil {
		return protocol.QueuedTurn{}, err
	}
	output, err := run.command(run.ctx, "submit_turn", protocol.CommandSubmitTurnPayload{Session: run.session.ID, ExpectedRevision: run.session.Revision, Message: message(text)})
	if err != nil {
		return protocol.QueuedTurn{}, err
	}
	return run.recordTurn(output, "queued_turn")
}

func (run *caseRun) recordTurn(output protocol.Output, kind string) (protocol.QueuedTurn, error) {
	turn, err := outputAs[protocol.QueuedTurn](output, kind)
	if err != nil {
		return turn, err
	}
	if turn.Session != run.session.ID {
		return turn, fail("Admitted turn belongs to another session")
	}
	run.turns = append(run.turns, turn)
	run.result.Turns = append(run.result.Turns, turn.ID.String())
	run.evidence("turn", turn.ID.String())
	return turn, nil
}

func (run *caseRun) ask(text string) (turnView, error) {
	turn, err := run.submit(text)
	if err != nil {
		return turnView{}, err
	}
	view, err := run.wait(turn.ID, nil)
	if err != nil {
		return view, err
	}
	if view.run.Status != protocol.ConversationStatusCompleted {
		if view.run.Error != nil && slices.Contains([]protocol.ErrorCode{protocol.ErrorCodeNotConfigured, protocol.ErrorCodeUnavailable, protocol.ErrorCodePermissionDenied}, view.run.Error.Code) {
			return view, blocked("Selected model execution is unavailable: " + string(view.run.Error.Code))
		}
		return view, fail("Model turn did not complete: " + string(view.run.Status))
	}
	if err := run.assertCapture(turn); err != nil {
		return view, err
	}
	return view, nil
}

func (run *caseRun) assertCapture(turn protocol.QueuedTurn) error {
	capture, err := run.capture(turn.ID)
	if err != nil {
		return err
	}
	if capture.ID != turn.ID || capture.Request != turn.Request || capture.Config.Provider != run.config.Provider || capture.Config.Model != run.config.Model || capture.Roles.Execution != protocol.ConversationExecutionModel {
		return fail("Turn capture differs from the selected model or admitted request")
	}
	expectedEffort, expectedErr := json.Marshal(run.config.Effort)
	admittedEffort, admittedErr := json.Marshal(turn.Config.Effort)
	capturedEffort, capturedErr := json.Marshal(capture.Config.Effort)
	if expectedErr != nil || admittedErr != nil || capturedErr != nil || !bytes.Equal(expectedEffort, admittedEffort) || !bytes.Equal(expectedEffort, capturedEffort) {
		return fail("Admitted or captured reasoning effort differs from the selected effort")
	}
	run.evidence("capture", "Selected provider/model and request identity preserved")
	run.evidence("capture_effort", "kind="+capture.Config.Effort.Kind+" encoded="+string(capturedEffort))
	return nil
}

func (run *caseRun) seed(path, text string) error {
	_, err := run.command(run.ctx, "write_file", protocol.CommandWriteFilePayload{Worktree: run.session.Worktree, Path: path, Text: text})
	return err
}

func (run *caseRun) read(path string) (protocol.FileContent, error) {
	output, err := run.command(run.ctx, "read_file", protocol.CommandReadFilePayload{Worktree: run.session.Worktree, Path: path})
	if err != nil {
		return protocol.FileContent{}, err
	}
	return outputAs[protocol.FileContent](output, "file_content")
}

func (run *caseRun) fileEquals(path, expected string) error {
	content, err := run.read(path)
	if err != nil {
		return err
	}
	if content.Truncated || content.Text != expected {
		return fail("Fixture file content differs from the expected artifact: " + path)
	}
	run.evidence("artifact", path+" matches expected content")
	return nil
}

func (run *caseRun) cleanup() error {
	sessions := make([]protocol.SessionID, 0, len(run.owned))
	for _, session := range run.owned {
		sessions = append(sessions, session.id)
	}
	return Archive(run.ctx, run.controller, run.directory, sessions)
}

func (run *caseRun) requireText(view turnView, text string) error {
	if !strings.Contains(view.text(), text) {
		return fail("Assistant response is missing the expected fixture marker")
	}
	run.evidence("text", "Expected fixture marker is present")
	return nil
}

func (run *caseRun) requireTool(view turnView, name string) (toolCall, protocol.ToolDetail, error) {
	for _, call := range view.tools() {
		if call.name != name {
			continue
		}
		detail, err := run.detail(call)
		return call, detail, err
	}
	return toolCall{}, protocol.ToolDetail{}, fail("Model did not execute the required tool: " + name)
}

func (run *caseRun) detail(call toolCall) (protocol.ToolDetail, error) {
	if call.summary == nil || call.reference.Revision == "" {
		return protocol.ToolDetail{}, fail("Tool has no inspectable canonical detail: " + call.name)
	}
	// HasDetails controls presentation. Pending approvals and questions, and
	// summarized read results, still have revision-checked canonical details.
	detail, err := run.controller.ReadToolDetail(run.ctx, call.reference)
	if err == nil {
		run.evidence("tool", call.name+" canonical arguments and result inspected")
	}
	return detail, err
}

func arguments(detail protocol.ToolDetail) map[string]json.RawMessage {
	var values map[string]json.RawMessage
	_ = json.Unmarshal(detail.Arguments, &values)
	return values
}

func argumentString(detail protocol.ToolDetail, key string) string {
	var value string
	_ = json.Unmarshal(arguments(detail)[key], &value)
	return value
}

func (run *caseRun) assertToolPath(view turnView, name, path string) error {
	call, detail, err := run.requireTool(view, name)
	if err != nil {
		return err
	}
	if call.summary.Failure != nil || detail.Result == nil || argumentString(detail, "path") != path {
		return fail(fmt.Sprintf("%s did not successfully address fixture %s", name, path))
	}
	return nil
}
