package live

import (
	"cmp"
	"fmt"
	"maps"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Terminal: the settings host's shell and environment, read
// when the page shows and saved whole on every change.

// terminalSettingsState is a Node's terminal settings as the page edits
// them. shown and visible are the local runtime's: the Node whose settings
// the page holds, and whether the page showed last frame.
type terminalSettingsState struct {
	model     settings.Stored[settings.TerminalHost]
	choices   []viewmodel.Shell
	shells    []protocol.TerminalShell
	saved     protocol.TerminalSettings
	uncertain *protocol.Request
	released  *releasedTerminalSave
	observed  uint64
	known     bool
	reading   bool
	failed    bool
	stale     bool
	shown     *Runtime
	visible   bool
}

// A released request is no longer inspectable. Neither its original proposal
// nor the draft submitted to resolve it gets rebased by a settings reread.
type releasedTerminalSave struct {
	proposal protocol.TerminalSettings
	draft    protocol.TerminalSettings
	unknown  bool
}

// terminalSettingsFrame stores the outgoing Node's draft and displays the
// selected Node's. It reads unknown or stale settings while Terminal shows.
func (r *Runtime) terminalSettingsFrame() {
	root := r.localRuntime()
	state := &root.terminalSettings
	m := root.App.Settings
	if !m.HasSection(settings.Terminal) {
		return
	}
	owner := root.managedSettingsNode()
	if previous := state.shown; previous != nil {
		previous.terminalSettings.model = m.Host
	}
	if owner != state.shown {
		m.CloseTerminalEditor()
	}
	visible := root.App.Page == shell.SettingsPage && m.Section == settings.Terminal
	entered := visible && (!state.visible || owner != state.shown)
	state.shown, state.visible = owner, visible
	if owner == nil {
		m.Host, m.Shells, m.SaveTerminal = settings.Stored[settings.TerminalHost]{}, nil, nil
		return
	}
	s := &owner.terminalSettings
	if entered {
		s.failed = false
	}
	if visible && !s.reading && !s.model.Pending {
		revision := owner.snapshot().TerminalSettingsRevision
		if (!s.known || s.stale) && !s.failed || s.known && revision > max(s.observed, s.saved.Revision) {
			owner.readTerminalSettings()
		}
	}
	m.Host, m.Shells, m.SaveTerminal = s.model, s.choices, nil
	if s.known {
		m.SaveTerminal = owner.saveTerminalSettings
	}
}

// terminalModel is where the Node's draft lives now: the page while it
// shows this Node, the Node's own copy otherwise.
func (r *Runtime) terminalModel() *settings.Stored[settings.TerminalHost] {
	root := r.localRuntime()
	if root.terminalSettings.shown == r {
		return &root.App.Settings.Host
	}
	return &r.terminalSettings.model
}

// readTerminalSettings reads the Node's settings, then the shells it
// offers. A failure is a toast and is not tried again until the page is
// entered anew.
func (r *Runtime) readTerminalSettings() {
	state := &r.terminalSettings
	if state.reading || r.ctx != nil && r.ctx.Err() != nil {
		return
	}
	state.reading, state.stale = true, false
	state.observed = r.snapshot().TerminalSettingsRevision
	state.reading = r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_terminal_settings"})
		value, ok := detail[protocol.TerminalSettings](output.Data)
		if err == nil && (output.Kind != "terminal_settings" || !ok) {
			err = fmt.Errorf("unexpected terminal settings response")
		}
		var shells []protocol.TerminalShell
		if err == nil {
			output, err = r.controller.Do(r.ctx, protocol.Command{Kind: "list_shells"})
			shells, ok = detail[[]protocol.TerminalShell](output.Data)
			if err == nil && (output.Kind != "shells" || !ok) {
				err = fmt.Errorf("unexpected shells response")
			}
		}
		r.enqueue(func(c *ui.Context) {
			state.reading = false
			if err != nil {
				state.failed = true
				if root := r.localRuntime(); root.terminalSettings.shown == r && root.terminalSettings.visible {
					settings.Report(c, kit.Failed, settings.TerminalMessages)
				}
				return
			}
			r.acceptTerminalRead(value, shells)
		})
	})
}

// acceptTerminalRead takes a read's answer, unless it is older than the
// settings held: such a read was answered before a save whose result was
// accepted since, and would take the page back to what that save replaced.
// The shells it lists are still the Node's.
func (r *Runtime) acceptTerminalRead(value protocol.TerminalSettings, shells []protocol.TerminalShell) {
	if state := &r.terminalSettings; state.known && value.Revision < state.saved.Revision {
		value = state.saved
	}
	r.acceptTerminalSettings(value, shells)
}

// acceptTerminalSettings takes what the Node holds. A draft that differs
// from what was saved (a save failed, conflicted or is in doubt) is kept,
// its shell found again among the new choices; otherwise the draft
// follows the Node.
func (r *Runtime) acceptTerminalSettings(value protocol.TerminalSettings, shells []protocol.TerminalShell) {
	state := &r.terminalSettings
	model := r.terminalModel()
	dirty := state.known && !sameTerminalHost(model.Draft, model.Saved)
	var keep []viewmodel.Shell
	if dirty {
		keep = []viewmodel.Shell{terminalShell(model.Draft, state.choices, state.saved)}
	}
	choices := terminalShells(shells, append(keep, viewmodel.Shell{Path: value.Shell, Args: value.ShellArguments})...)
	host := terminalHost(value, choices)
	if dirty {
		model.Draft.Shell = shellIndex(choices, keep[0])
	} else {
		model.Draft = host
	}
	model.Saved, model.Revision = host, value.Revision
	state.saved, state.choices, state.known, state.failed = value, choices, true, false
	state.shells = shells
}

// saveTerminalSettings submits the shown Node's draft at the revision it was read.
func (r *Runtime) saveTerminalSettings() {
	state := &r.terminalSettings
	model := r.terminalModel()
	root := r.localRuntime()
	if !root.terminalSettings.visible || root.terminalSettings.shown != r || root.managedSettingsNode() != r {
		return
	}
	if model.Pending || !state.known || !r.controller.Connected() || root.runtimeFor(r.controller.Target().String()) != r {
		return
	}
	draft := terminalRequest(model.Draft, model.Revision, state.choices, state.saved)
	previous := state.uncertain
	if released := state.released; previous == nil && released != nil && (released.unknown || draft.Revision != released.proposal.Revision) &&
		(sameTerminalValues(draft, released.proposal) || sameTerminalValues(draft, released.draft)) {
		outcome := kit.Conflict
		if released.unknown {
			outcome = kit.Uncertain
		}
		r.enqueue(func(c *ui.Context) { settings.Report(c, outcome, settings.TerminalMessages) })
		return
	}
	model.Pending = r.async(func() {
		output, request, err := r.writeTerminalSettings(draft, previous)
		r.enqueue(func(c *ui.Context) {
			state.uncertain = nil
			if client.Retained(err) {
				state.uncertain = request
			} else if client.Released(err) {
				proposal := request.Command.Data.(protocol.TerminalSettings)
				state.released = &releasedTerminalSave{proposal: proposal, draft: draft, unknown: faultCode(err) == protocol.ErrorCodeOutcomeUnknown}
			}
			model := r.terminalModel()
			model.Pending = false
			value, valid := detail[protocol.TerminalSettings](output.Data)
			if err == nil && valid && output.Kind == "terminal_settings" {
				state.released = nil
				state.observed = max(state.observed, value.Revision)
				if sameTerminalValues(terminalRequest(model.Draft, model.Revision, state.choices, state.saved), value) {
					model.Draft = model.Saved
				}
				r.acceptTerminalRead(value, state.shells)
				settings.Report(c, kit.Done, settings.TerminalMessages)
				return
			}
			state.stale = true
			outcome := kit.Failed
			switch faultCode(err) {
			case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeConflict:
				outcome = kit.Conflict
			case protocol.ErrorCodeOutcomeUnknown:
				outcome = kit.Uncertain
			}
			settings.Report(c, outcome, settings.TerminalMessages)
		})
	})
}

func (r *Runtime) writeTerminalSettings(draft protocol.TerminalSettings, previous *protocol.Request) (protocol.Output, *protocol.Request, error) {
	if previous != nil {
		resolution, err := r.controller.Resolve(r.ctx, *previous)
		if err != nil {
			return protocol.Output{}, previous, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "terminal settings outcome could not be confirmed"}
		}
		output, err := resolution.Result()
		if client.Retained(err) || client.Released(err) {
			return output, previous, err
		}
		saved, ok := detail[protocol.TerminalSettings](output.Data)
		if err != nil || !ok || output.Kind != "terminal_settings" || sameTerminalValues(draft, saved) {
			return output, nil, err
		}
		// A further edit may build on our confirmed save, not on another controller's revision.
		if saved.Revision < draft.Revision {
			return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "terminal settings changed"}
		}
		draft.Revision = saved.Revision
	}
	request := r.controller.Prepare(protocol.Command{Kind: "save_terminal_settings", Data: draft})
	output, err := r.controller.Settle(r.ctx, request)
	if client.Retained(err) || client.Released(err) {
		return output, &request, err
	}
	return output, nil, err
}

func sameTerminalValues(a, b protocol.TerminalSettings) bool {
	return a.Shell == b.Shell && slices.Equal(a.ShellArguments, b.ShellArguments) && maps.Equal(a.Environment, b.Environment)
}

// terminalShells are the select's choices: the host's default, the shells
// the Node offers once each, and then any shell in use the Node did not
// list.
func terminalShells(offered []protocol.TerminalShell, used ...viewmodel.Shell) []viewmodel.Shell {
	choices := []viewmodel.Shell{{}}
	add := func(s viewmodel.Shell) {
		if s.Path != "" && shellIndex(choices, s) == 0 {
			choices = append(choices, viewmodel.Shell{Path: s.Path, Args: slices.Clone(s.Args)})
		}
	}
	for _, s := range offered {
		add(viewmodel.Shell{Path: s.Path, Args: s.Arguments})
	}
	for _, s := range used {
		add(s)
	}
	return choices
}

// shellIndex is s among choices, or the default's 0.
func shellIndex(choices []viewmodel.Shell, s viewmodel.Shell) int {
	return max(slices.IndexFunc(choices, func(o viewmodel.Shell) bool {
		return o.Path == s.Path && slices.Equal(o.Args, s.Args)
	}), 0)
}

// terminalHost is the page's view of the Node's settings: the variables by
// name and the shell's place among choices.
func terminalHost(value protocol.TerminalSettings, choices []viewmodel.Shell) settings.TerminalHost {
	host := settings.TerminalHost{Shell: shellIndex(choices, viewmodel.Shell{Path: value.Shell, Args: value.ShellArguments})}
	for key, v := range value.Environment {
		host.Env = append(host.Env, viewmodel.EnvVar{Key: key, Value: v})
	}
	slices.SortFunc(host.Env, func(a, b viewmodel.EnvVar) int { return cmp.Compare(a.Key, b.Key) })
	return host
}

// terminalShell is the shell a draft chose; a choice out of range keeps
// the saved shell.
func terminalShell(draft settings.TerminalHost, choices []viewmodel.Shell, saved protocol.TerminalSettings) viewmodel.Shell {
	if draft.Shell < 0 || draft.Shell >= len(choices) {
		return viewmodel.Shell{Path: saved.Shell, Args: saved.ShellArguments}
	}
	return choices[draft.Shell]
}

// terminalRequest is the draft as the Node saves it, at revision.
func terminalRequest(draft settings.TerminalHost, revision uint64, choices []viewmodel.Shell, saved protocol.TerminalSettings) protocol.TerminalSettings {
	chosen := terminalShell(draft, choices, saved)
	request := protocol.TerminalSettings{Revision: revision, Shell: chosen.Path, ShellArguments: slices.Clone(chosen.Args), Environment: map[string]string{}}
	if request.ShellArguments == nil {
		request.ShellArguments = []string{}
	}
	for _, v := range draft.Env {
		request.Environment[v.Key] = v.Value
	}
	return request
}

func sameTerminalHost(a, b settings.TerminalHost) bool {
	return a.Shell == b.Shell && slices.Equal(a.Env, b.Env)
}
