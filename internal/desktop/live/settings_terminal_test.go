package live

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// checkTerminalSettingsHelpers is the smoke of the terminal settings'
// projection: the shell choices, the page's view of the Node's settings
// and the request a draft saves.
func checkTerminalSettingsHelpers(t *testing.T) {
	t.Helper()
	offered := []protocol.TerminalShell{
		{Path: "/bin/zsh", Arguments: []string{"-l"}},
		{Path: "/bin/zsh", Arguments: []string{"-l"}},
		{Path: "/bin/bash", Arguments: []string{}},
		{Path: ""},
	}
	saved := protocol.TerminalSettings{Revision: 4, Shell: "/opt/fish", ShellArguments: []string{"-i"}, Environment: map[string]string{"ZED": "1", "ALPHA": "a b"}}
	choices := terminalShells(offered, viewmodel.Shell{Path: saved.Shell, Args: saved.ShellArguments})
	want := []viewmodel.Shell{{}, {Path: "/bin/zsh", Args: []string{"-l"}}, {Path: "/bin/bash"}, {Path: "/opt/fish", Args: []string{"-i"}}}
	if len(choices) != len(want) {
		t.Fatalf("choices = %+v, want %+v", choices, want)
	}
	for i := range want {
		if shellIndex(choices, want[i]) != i {
			t.Errorf("choice %d = %+v, want %+v", i, choices[i], want[i])
		}
	}

	host := terminalHost(saved, choices)
	env := []viewmodel.EnvVar{{Key: "ALPHA", Value: "a b"}, {Key: "ZED", Value: "1"}}
	if host.Shell != 3 || !reflect.DeepEqual(host.Env, env) {
		t.Errorf("host = %+v", host)
	}
	if !sameTerminalHost(host, settings.TerminalHost{Env: env, Shell: 3}) || sameTerminalHost(host, settings.TerminalHost{Env: env}) {
		t.Error("drafts compare wrongly")
	}

	// A draft's variables and shell go out at the revision read.
	draft := settings.TerminalHost{Env: append(env, viewmodel.EnvVar{Key: "NEW", Value: ""}), Shell: 1}
	request := terminalRequest(draft, saved.Revision, choices, saved)
	if request.Revision != 4 || request.Shell != "/bin/zsh" || !reflect.DeepEqual(request.ShellArguments, []string{"-l"}) ||
		!reflect.DeepEqual(request.Environment, map[string]string{"ALPHA": "a b", "ZED": "1", "NEW": ""}) {
		t.Errorf("request = %+v", request)
	}
	// The default is no program; a choice out of range keeps the saved one.
	if request = terminalRequest(settings.TerminalHost{}, 4, choices, saved); request.Shell != "" || len(request.ShellArguments) != 0 || len(request.Environment) != 0 {
		t.Errorf("default request = %+v", request)
	}
	if request = terminalRequest(settings.TerminalHost{Shell: 9}, 4, choices, saved); request.Shell != "/opt/fish" || !reflect.DeepEqual(request.ShellArguments, []string{"-i"}) {
		t.Errorf("out of range request = %+v", request)
	}
	if _, err := request.MarshalJSON(); err != nil {
		t.Errorf("request does not encode: %v", err)
	}
}
