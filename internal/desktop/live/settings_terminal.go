package live

import (
	"cmp"
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Terminal: the selected host's shell and environment, read
// when the page shows and saved whole on every change.

// terminalSettingsState is a Node's terminal settings as the page edits
// them. shown and visible are the local runtime's: the Node whose settings
// the page holds, and whether the page showed last frame.
type terminalSettingsState struct {
	model    settings.Stored[settings.TerminalHost]
	choices  []viewmodel.Shell
	shells   []protocol.TerminalShell
	saved    protocol.TerminalSettings
	observed uint64
	known    bool
	reading  bool
	failed   bool
	stale    bool
	shown    *Runtime
	visible  bool
}

// terminalSettingsFrame keeps the page on the selected host's Node: it
// takes back the draft the page edited, shows the Node's, and reads its
// settings when they are unknown, a save left them in doubt, or the Node
// reports a newer revision. Nothing is read while another page shows.
func (r *Runtime) terminalSettingsFrame() {
	root := r.localRuntime()
	state := &root.terminalSettings
	m := root.App.Settings
	owner := root.runtimeFor(root.App.Host)
	if previous := state.shown; previous != nil {
		previous.terminalSettings.model = m.Host
		if m.TerminalEditing() {
			owner = previous
		}
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
				settings.Report(c, kit.Failed, settings.TerminalMessages)
				return
			}
			r.acceptTerminalSettings(value, shells)
		})
	})
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

// saveTerminalSettings saves the page's draft at the revision it was read
// at. A request whose outcome is unknown is retained and sent again by the
// same draft; a failed or conflicting save keeps the draft and reads the
// Node's settings again.
func (r *Runtime) saveTerminalSettings() {
	state := &r.terminalSettings
	model := r.terminalModel()
	if model.Pending || !state.known {
		return
	}
	model.Pending = true
	request := terminalRequest(model.Draft, model.Revision, state.choices, state.saved)
	r.saveSetting(protocol.Command{Kind: "save_terminal_settings", Data: request}, settings.TerminalMessages, func(output protocol.Output, ok bool) {
		model := r.terminalModel()
		model.Pending = false
		value, valid := detail[protocol.TerminalSettings](output.Data)
		if !ok || !valid || output.Kind != "terminal_settings" {
			state.stale = true
			return
		}
		state.observed = value.Revision
		model.Draft = model.Saved
		r.acceptTerminalSettings(value, state.shells)
	})
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
