package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"runtime/debug"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const liveScope = "This is an explicitly requested Sailry acceptance case. Work only in this conversation's dedicated workspace. Do not access credentials, environment variables, system files, network, other sessions or user repositories. Do not install anything or delegate. Use only the tools named in the task. Keep the final reply brief unless the task requests a longer streaming sample.\n\n"

type liveScenario struct {
	id, title string
	tool      bool
	run       func(*liveWindow) error
}

type liveIssue struct{ status, detail string }

func (issue *liveIssue) Error() string { return issue.detail }

func unavailable(detail string) error { return &liveIssue{"blocked", detail} }

// RunLive exercises the real live adapter through headless UI input. The caller
// owns Client and the existing Node; profile is a dedicated acceptance preference
// directory, never the user's controller profile. Client commands only prepare,
// inspect and clean up sessions: messages are always sent through the composer.
func RunLive(ctx context.Context, controller *client.Client, config *protocol.SessionConfig, emit func(core.CaseResult), profile string) core.Report {
	return RunLiveWithOptions(ctx, controller, core.Options{Config: config, ReportDirectory: profile}, emit, profile)
}

// RunLiveWithOptions records real Client snapshots and temporary session
// ownership in ReportDirectory. Profile contains only isolated UI preferences.
// Explicit case selections may also name backend or fixture cases; only
// matching E2E cases run here, and an empty selection runs all live cases.
func RunLiveWithOptions(ctx context.Context, controller *client.Client, options core.Options, emit func(core.CaseResult), profile string) core.Report {
	report := core.Report{Version: 1, Suite: "e2e", Started: time.Now().UTC(), Cases: []core.CaseResult{}}
	if controller != nil {
		report.Node = controller.Target()
	}
	cases := []liveScenario{
		{"E2E01", "Send through the live composer and observe streamed text", false, liveStream},
		{"E2E02", "Expand and copy a real tool result", true, liveTool},
		{"E2E03", "Stop a real reply through the live composer", false, liveStop},
	}
	if len(options.Cases) > 0 {
		cases = slices.DeleteFunc(cases, func(scene liveScenario) bool { return !slices.Contains(options.Cases, scene.id) })
	}
	if len(cases) == 0 {
		report.Finished = time.Now().UTC()
		return report
	}
	selected, problem := liveConfig(controller, options.Config, profile)
	if options.ReportDirectory == "" {
		problem = unavailable("A dedicated report directory is required for live UI acceptance")
	} else if options.CaseTimeout < 0 {
		problem = unavailable("The live UI case timeout must be positive")
	}
	if problem == nil {
		report.Provider, report.Model = selected.Provider.String(), selected.Model
		report.Effort = &selected.Effort
	}
	stop := false
	for _, scene := range cases {
		result := core.CaseResult{ID: scene.id, Group: "e2e", Title: scene.title}
		switch {
		case ctx.Err() != nil:
			result.Status, result.Detail = "canceled", "Acceptance canceled before this case started"
		case stop:
			result.Status, result.Detail = "blocked", "A previous case has unconfirmed cleanup or admission"
		case problem != nil:
			result.Status, result.Detail = "blocked", problem.Error()
		case scene.tool && !commandsEnabled(controller.Snapshot()):
			result.Status, result.Detail = "skipped", "The commands package is disabled, unavailable or incompatible"
		default:
			result, stop = runLiveCase(ctx, controller, selected, profile, options, scene, emit)
		}
		report.Cases = append(report.Cases, result)
		if emit != nil {
			emit(result)
		}
	}
	report.Finished = time.Now().UTC()
	return report
}

func liveConfig(controller *client.Client, requested *protocol.SessionConfig, profile string) (protocol.SessionConfig, error) {
	if controller == nil || profile == "" {
		return protocol.SessionConfig{}, unavailable("An existing Client and dedicated acceptance preference directory are required")
	}
	snapshot := controller.Snapshot()
	if requested == nil {
		requested = snapshot.Defaults.Config
	}
	if requested == nil || requested.Model == "" {
		return protocol.SessionConfig{}, unavailable("Choose a configured provider and model before live acceptance")
	}
	config, err := copyLiveValue[protocol.SessionConfig](requested)
	if err != nil {
		return config, unavailable("The selected model configuration could not be copied")
	}
	valid := false
	for _, provider := range snapshot.Providers {
		if provider.ID == config.Provider && provider.Enabled && (provider.DefaultModel == config.Model || slices.ContainsFunc(provider.Models, func(model protocol.ConversationModel) bool { return model.ID == config.Model })) {
			valid = true
			if config.Credential == nil {
				config.Credential = provider.Credential
			}
		}
	}
	if !valid {
		return config, unavailable("The selected model is not enabled in this execution Node")
	}
	config.Assistant, config.Resource = nil, nil
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	if config.Effort.Kind == "" {
		config.Effort = protocol.Effort{Kind: "default"}
	}
	return config, nil
}

func commandsEnabled(snapshot protocol.Snapshot) bool {
	return slices.ContainsFunc(snapshot.Plugins, func(plugin protocol.PluginSummary) bool {
		return plugin.Name == "commands" && plugin.Enabled && plugin.Compatibility.Kind == "compatible"
	})
}

type liveWindow struct {
	*window
	controller      *client.Client
	runtime         *live.Runtime
	config          protocol.SessionConfig
	session         protocol.Session
	owned           bool
	unconfirmed     bool
	wake            chan struct{}
	lastFrame       time.Time
	turn            protocol.TurnID
	progress        func(core.CaseResult)
	reportDirectory string
	recorder        *core.Recorder
	started         time.Time
	recordErr       error
}

func runLiveCase(ctx context.Context, controller *client.Client, config protocol.SessionConfig, profile string, options core.Options, scene liveScenario, emit func(core.CaseResult)) (core.CaseResult, bool) {
	started := time.Now()
	result := core.CaseResult{ID: scene.id, Group: "e2e", Title: scene.title, Status: "failed",
		Evidence: []core.Evidence{{Kind: "scope", Value: "Headless MyGo input; real live adapter and existing Node; isolated unassigned session; no native window"}}}
	timeout := options.CaseTimeout
	if timeout == 0 {
		timeout = 2 * time.Minute
	}
	scoped, cancel := context.WithTimeout(ctx, timeout)
	w := &liveWindow{window: &window{ctx: scoped, result: &result}, controller: controller, config: config, wake: make(chan struct{}, 1), progress: emit,
		reportDirectory: options.ReportDirectory, started: started}
	err := w.run(profile, scene.run)
	if w.runtime != nil {
		if closeErr := w.runtime.Close(); err == nil {
			err = closeErr
		}
	}
	if recordErr := w.flushRecording(); recordErr != nil {
		w.evidence("recording", recordErr.Error())
		err = errors.Join(err, recordErr)
	}
	cancel()
	cleanupErr := w.cleanup()
	if w.owned && w.reportDirectory != "" {
		if recording, recordErr := core.ReadRecording(w.reportDirectory, w.session.ID); recordErr == nil {
			w.evidence("recording", core.RecordingPath(w.reportDirectory, w.session.ID))
			w.evidence("recording_frames", fmt.Sprintf("Retained %d frames; truncated=%t; dropped=%d", len(recording.Frames), recording.Truncated, recording.DroppedFrames))
		} else {
			w.evidence("recording", "The saved live UI recording could not be read")
			err = errors.Join(err, liveFail("The saved live UI recording could not be read"))
		}
	}
	result.Status, result.Detail = "passed", "All live UI assertions passed"
	if err != nil {
		var issue *liveIssue
		switch {
		case ctx.Err() != nil:
			result.Status, result.Detail = "canceled", "Acceptance canceled; cleanup requested for its session"
		case errors.As(err, &issue):
			result.Status, result.Detail = issue.status, issue.detail
		default:
			result.Status, result.Detail = "failed", liveError(err)
		}
	}
	if cleanupErr != nil {
		result.Status, result.Detail = "blocked", "Cleanup is unconfirmed; inspect the recorded session before running more cases"
		w.evidence("cleanup", liveError(cleanupErr))
	}
	if w.unconfirmed && cleanupErr == nil {
		result.Status, result.Detail = "blocked", "Admission was unconfirmed; inspect its recorded request before running more cases"
	}
	result.Duration = time.Since(started)
	result.Uncertain = cleanupErr != nil || w.unconfirmed
	return result, result.Uncertain
}

func (w *liveWindow) run(profile string, run func(*liveWindow) error) (err error) {
	defer func() {
		if recover() != nil {
			err = liveFail("Live UI interaction panicked")
			w.evidence("stack", string(debug.Stack()))
		}
	}()
	if err = w.newSession(); err != nil {
		return err
	}
	if w.runtime, err = live.New(w.ctx, w.controller, profile); err != nil {
		return liveFail("The live runtime could not be initialized: " + err.Error())
	}
	w.app = w.runtime.App
	w.app.Effects = false
	afterFrame := w.app.Hooks.AfterFrame
	w.app.Hooks.AfterFrame = func() {
		if afterFrame != nil {
			afterFrame()
		}
		w.recordFrame()
	}
	// Select only our session before the first frame. If its sidebar event is
	// still in flight, OpenSession completes selection when it is projected.
	w.app.Sidebar.Selected = w.session.ID.String()
	w.runtime.OpenSession(w.session.ID.String())
	w.runtime.SetInvalidate(w.notify)
	w.tester = ui.NewTesterWith(w.app.View, 1280, 820, ui.TesterOptions{
		Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true},
	})
	w.tester.SetLocale("en")
	w.lastFrame = time.Now()
	lastSetup := ""
	if err = w.wait(func() (bool, error) {
		state, known := w.controller.Conversation(w.session.ID)
		ready, setup := w.readyForInput(state, known)
		if setup != lastSetup {
			w.evidence("ui_setup", setup)
			w.emitProgress()
			lastSetup = setup
		}
		return ready, nil
	}); err != nil {
		return err
	}
	return run(w)
}

func modelID(config protocol.SessionConfig) string {
	return config.Provider.String() + "/" + config.Model
}

func (w *liveWindow) readyForInput(state client.ConversationState, observed bool) (bool, string) {
	id := w.session.ID.String()
	cv := w.app.Data.Find(id)
	selected := w.app.Sidebar.Selected == id
	modelSelected := cv != nil && w.app.Composer(id).Model == modelID(w.config)
	// Client can become ready after the preceding frame read it. Wait for
	// that frame's projection too, so typing starts in the final text field.
	projected := cv != nil && !cv.Session.Loading && cv.Session.Connection == conversation.Connected
	field, present := w.tester.AccessByRole(ui.RoleTextField, label("composer"))
	editable := present && field.Bounds.W > 0 && field.Bounds.H > 0 && !field.Disabled && !field.ReadOnly
	detail := fmt.Sprintf("Session present=%t, selected=%t, observed=%t, connected=%t, loading=%t, model selected=%t, projected ready=%t, composer editable=%t",
		cv != nil, selected, observed, state.Connected, state.Loading, modelSelected, projected, editable)
	return selected && observed && state.Connected && !state.Loading && modelSelected && projected && editable, detail
}

func (w *liveWindow) notify() {
	select {
	case w.wake <- struct{}{}:
	default:
	}
}

// Advance only lets UI timers catch up. Real Node events and the real context
// deadline, rather than Tester's simulated clock, drive model observations.
func (w *liveWindow) frame() {
	now := time.Now()
	w.tester.Advance(now.Sub(w.lastFrame))
	w.lastFrame = now
	w.tester.Frame()
}

func (w *liveWindow) recordFrame() {
	if w.recorder == nil || w.recordErr != nil {
		return
	}
	state, known := w.controller.Conversation(w.session.ID)
	if !known || state.Snapshot.Page.Session != w.session.ID {
		return
	}
	// Recorder deduplicates complete snapshots, so draft deltas are retained
	// even when they do not advance the durable sequence.
	if err := w.recorder.Append(core.RecordingFrame{At: time.Since(w.started), Snapshot: state.Snapshot}); err != nil {
		w.recordErr = liveFail("The live UI recording could not capture a frame")
		return
	}
}

func (w *liveWindow) flushRecording() error {
	if w.recorder == nil {
		return w.recordErr
	}
	w.recordFrame()
	if err := w.recorder.Flush(); err != nil {
		return liveFail("The live UI recording could not be saved")
	}
	return w.recordErr
}

func (w *liveWindow) wait(assert func() (bool, error)) error {
	ticker := time.NewTicker(40 * time.Millisecond)
	defer ticker.Stop()
	for {
		if err := w.ctx.Err(); err != nil {
			return err
		}
		w.frame()
		if w.recordErr != nil {
			return w.recordErr
		}
		if state, known := w.controller.Conversation(w.session.ID); known {
			if state.Err != nil {
				return state.Err
			}
			if state.Removed {
				return liveFail("The acceptance session was removed while observed")
			}
			for _, approval := range state.Snapshot.Page.Approvals {
				if approval.State == protocol.ConversationApprovalStatePending {
					return unavailable("An unexpected approval blocked the case; no automatic approval was sent")
				}
			}
			for _, question := range state.Snapshot.Page.Questions {
				if question.State.Kind == "pending" {
					return unavailable("An unexpected question blocked the case; no automatic answer was sent")
				}
			}
		}
		done, err := assert()
		if done || err != nil {
			return err
		}
		select {
		case <-w.ctx.Done():
			return w.ctx.Err()
		case <-w.wake:
		case <-ticker.C:
		}
	}
}

func (w *liveWindow) newSession() error {
	output, err := w.command(w.ctx, "create_session", protocol.CommandCreateSessionPayload{Config: &w.config})
	if err != nil {
		return err
	}
	if w.session, err = liveOutput[protocol.Session](output, "session"); err != nil {
		return err
	}
	w.owned = true
	w.result.Session = w.session.ID.String()
	w.result.OwnedSessions = []string{w.session.ID.String()}
	w.emitProgress()
	if w.reportDirectory != "" {
		if err := core.Own(w.reportDirectory, w.controller.Target(), w.session.ID); err != nil {
			return liveFail("The temporary acceptance session could not be recorded for cleanup")
		}
		w.recorder, err = core.NewRecorder(w.reportDirectory, w.controller.Target(), w.session.ID)
		if err != nil {
			return liveFail("The live UI recording could not be initialized")
		}
	}
	if w.session.Project != nil || w.session.Config.Resource != nil || w.session.Config.Assistant != nil {
		return liveFail("The acceptance session was assigned to a non-fixture resource")
	}
	output, err = w.command(w.ctx, "set_session_team", protocol.CommandSetSessionExecutionPayload{Session: w.session.ID, ExpectedRevision: w.session.Revision, Execution: protocol.ConversationExecutionModel})
	if err != nil {
		return err
	}
	next, err := liveOutput[protocol.Session](output, "session")
	if err != nil {
		return err
	}
	if next.ID != w.session.ID {
		return liveFail("The model selection returned a different acceptance session")
	}
	w.session = next
	output, err = w.command(w.ctx, "rename_session", protocol.CommandRenameSessionPayload{Session: w.session.ID, ExpectedRevision: w.session.Revision, Title: "Acceptance " + w.result.ID})
	if err != nil {
		return err
	}
	next, err = liveOutput[protocol.Session](output, "session")
	if err != nil {
		return err
	}
	if next.ID != w.session.ID {
		return liveFail("The title update returned a different acceptance session")
	}
	w.session = next
	w.evidence("setup", "Created and named an isolated single-model session")
	return nil
}

func (w *liveWindow) send(text string) error {
	if err := w.typeMessage(liveScope + text); err != nil {
		return err
	}
	if w.app.Composer(w.session.ID.String()).Draft != liveScope+text {
		w.recordComposerInput(liveScope + text)
		return liveFail("Typing into the live composer did not preserve the fixture message")
	}
	// Model/session projection can still be settling after the input frame.
	// Observe the real enabled control before clicking, just as a user would.
	if err := w.wait(func() (bool, error) {
		button, err := w.control(ui.RoleButton, label("send"))
		return err == nil && !button.Disabled, err
	}); err != nil {
		return err
	}
	if err := w.button(label("send")); err != nil {
		return err
	}
	return w.wait(func() (bool, error) {
		state, known := w.controller.Conversation(w.session.ID)
		if !known {
			return false, nil
		}
		// Admitted input is a canonical user entry. Run.Input belongs to
		// queued edits and may be absent once the real UI send is admitted.
		for _, entry := range state.Snapshot.Page.Entries {
			if entry.Author != "user" {
				continue
			}
			var input string
			for _, part := range entry.Parts {
				if part.Kind == "text" {
					value, err := copyLiveValue[string](part.Data)
					if err != nil {
						return false, liveFail("The UI-sent input has invalid canonical text")
					}
					input += value
				}
			}
			if input != liveScope+text || !slices.ContainsFunc(state.Snapshot.Page.Runs, func(run protocol.ConversationRun) bool {
				return run.Session == w.session.ID && run.Turn == entry.Turn
			}) {
				continue
			}
			if w.turn != entry.Turn {
				w.turn = entry.Turn
				w.result.Turns = []string{entry.Turn.String()}
				w.emitProgress()
			}
			if w.app.Composer(w.session.ID.String()).Draft != "" {
				return false, nil
			}
			return true, nil
		}
		return false, nil
	})
}

func (w *liveWindow) emitProgress() {
	if w.progress == nil {
		return
	}
	result := *w.result
	result.Status, result.Detail = "running", "Observing the owned acceptance session"
	result.Turns = slices.Clone(result.Turns)
	result.OwnedSessions = slices.Clone(result.OwnedSessions)
	result.Evidence = slices.Clone(result.Evidence)
	w.progress(result)
}

func (w *liveWindow) uiTurn() *conversation.Turn {
	if cv := w.app.Data.Find(w.session.ID.String()); cv != nil {
		for i := range cv.Turns {
			if cv.Turns[i].ID == w.turn.String() {
				return &cv.Turns[i]
			}
		}
	}
	return nil
}

func (w *liveWindow) canonicalRun() (protocol.ConversationRun, bool) {
	state, known := w.controller.Conversation(w.session.ID)
	if known {
		for _, run := range state.Snapshot.Page.Runs {
			if run.Turn == w.turn {
				return run, true
			}
		}
	}
	return protocol.ConversationRun{}, false
}

func activeLive(status protocol.ConversationStatus) bool {
	return status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning || status == protocol.ConversationStatusStopping
}

func (w *liveWindow) capture() error {
	output, err := w.command(w.ctx, "read_turn_capture", protocol.CommandReadTurnCapturePayload{Session: w.session.ID, Turn: w.turn})
	if err != nil {
		return err
	}
	turn, err := liveOutput[protocol.QueuedTurn](output, "queued_turn")
	if err != nil {
		return err
	}
	if turn.Session != w.session.ID || turn.ID != w.turn || turn.Config.Provider != w.config.Provider || turn.Config.Model != w.config.Model || !reflect.DeepEqual(turn.Config.Effort, w.config.Effort) || turn.Roles.Execution != protocol.ConversationExecutionModel {
		return liveFail("The UI-sent turn capture differs from the selected session, model or effort")
	}
	w.evidence("capture", "The UI-sent turn preserved model "+turn.Config.Model+", effort "+turn.Config.Effort.Kind+" and single-model execution")
	w.evidence("request", turn.Request.String())
	return nil
}

func (w *liveWindow) command(ctx context.Context, kind string, data any) (protocol.Output, error) {
	if w.reportDirectory != "" {
		output, err := core.Execute(ctx, w.controller, w.reportDirectory, protocol.Command{Kind: kind, Data: data})
		if fault, known := liveFault(err); known && fault.Code == protocol.ErrorCodeOutcomeUnknown {
			w.unconfirmed = true
			w.evidence("request", fault.Message)
		}
		return output, err
	}
	request := w.controller.Prepare(protocol.Command{Kind: kind, Data: data})
	if request.Command.Durable() {
		w.evidence("request", kind+" "+request.ID.String())
	}
	output, err := w.controller.Execute(ctx, request)
	fault, known := liveFault(err)
	if err == nil || !known || fault.Code != protocol.ErrorCodeOutcomeUnknown {
		return output, err
	}
	inspect, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	outcome, inspectErr := w.controller.Outcome(inspect, request)
	if inspectErr == nil && outcome.Kind == "completed" {
		completed, decodeErr := copyLiveValue[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
		if decodeErr == nil && completed.Err != nil {
			return protocol.Output{}, completed.Err
		}
		if decodeErr == nil && completed.Ok != nil {
			return *completed.Ok, nil
		}
	}
	w.unconfirmed = true
	return protocol.Output{}, unavailable("Admission is unconfirmed; retain its request ID and do not resubmit")
}

func (w *liveWindow) cleanup() error {
	if w.reportDirectory != "" && (w.owned || w.unconfirmed) {
		var sessions []protocol.SessionID
		if w.owned {
			sessions = append(sessions, w.session.ID)
		}
		return core.Archive(w.ctx, w.controller, w.reportDirectory, sessions)
	}
	if !w.owned {
		return nil
	}
	ctx, cancel := context.WithTimeout(context.WithoutCancel(w.ctx), 15*time.Second)
	defer cancel()
	_, stopErr := w.command(ctx, "stop_commands", protocol.CommandStopCommandsPayload{Session: w.session.ID})
	requested := map[protocol.TurnID]bool{}
	for {
		output, err := w.controller.Do(ctx, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: w.session.ID, Limit: 100}})
		if err != nil {
			return errors.Join(stopErr, err)
		}
		history, err := liveOutput[protocol.ConversationHistory](output, "conversation")
		if err != nil {
			return errors.Join(stopErr, err)
		}
		active := false
		for _, run := range history.Page.Runs {
			if run.Session == w.session.ID && activeLive(run.Status) {
				active = true
				if !requested[run.Turn] {
					if _, err := w.command(ctx, "stop_turn", protocol.CommandStopTurnPayload{Turn: run.Turn}); err != nil {
						return errors.Join(stopErr, err)
					}
					requested[run.Turn] = true
				}
			}
		}
		if !active {
			w.evidence("cleanup", "The Node confirms no active turn remains in the owned session")
			return stopErr
		}
		select {
		case <-ctx.Done():
			return errors.Join(stopErr, ctx.Err())
		case <-time.After(100 * time.Millisecond):
		}
	}
}

func liveOutput[T any](output protocol.Output, kind string) (T, error) {
	var value T
	if output.Kind != kind {
		return value, liveFail("The Node returned an unexpected output kind; expected " + kind)
	}
	value, err := copyLiveValue[T](output.Data)
	if err != nil {
		return value, liveFail("The Node returned an invalid " + kind + " payload")
	}
	return value, nil
}

func copyLiveValue[T any](input any) (T, error) {
	var value T
	encoded, err := json.Marshal(input)
	if err == nil {
		err = json.Unmarshal(encoded, &value)
	}
	return value, err
}

func liveError(err error) string {
	var issue *liveIssue
	switch {
	case errors.As(err, &issue):
		return issue.detail
	case errors.Is(err, context.Canceled):
		return "Observation canceled"
	case errors.Is(err, context.DeadlineExceeded):
		return "Case deadline exceeded"
	default:
		if fault, known := liveFault(err); known {
			return "Node request failed: " + string(fault.Code)
		}
		// Protocol and provider errors can include private response bodies.
		// Scenario assertions provide their own safe, test-owned detail.
		return "Live UI acceptance operation failed"
	}
}

func liveFault(err error) (protocol.Fault, bool) {
	var value protocol.Fault
	if errors.As(err, &value) {
		return value, true
	}
	var pointer *protocol.Fault
	if errors.As(err, &pointer) && pointer != nil {
		return *pointer, true
	}
	return protocol.Fault{}, false
}
