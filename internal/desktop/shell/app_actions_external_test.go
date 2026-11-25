package shell_test

import (
	"encoding/json"
	"errors"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func TestAppActionFromAnotherModule(t *testing.T) {
	const id native.AppActionID = "fixture.schedule"
	const page native.Page = "fixture:schedule-controls"
	const button = "Schedule fixture"
	const input = `{"hour":9}`
	denied := &protocol.Fault{Code: protocol.ErrorCodePermissionDenied}
	scope := native.AppActionScope{Node: protocol.NodeID{1}, Worktree: protocol.WorktreeID{2}, Session: protocol.SessionID{3}}
	var attempts, executed, fallback int
	var refusal bool
	action := native.AppAction{ID: id, Run: func(_ *ui.Context, _ *native.App, source native.AppActionContext, data json.RawMessage) error {
		attempts++
		var value struct{ Hour int }
		if !source.Current() || source.AppActionScope != scope || json.Unmarshal(data, &value) != nil || value.Hour != 9 {
			t.Fatalf("wrong action source or input: %+v, %s", source.AppActionScope, data)
		}
		if refusal {
			return denied
		}
		executed++
		return nil
	}}
	var result error
	controls := func(assembly *native.Assembly) {
		assembly.WorkspacePages = append(assembly.WorkspacePages, native.NativePage{
			Page: page, Feature: native.Feature{ID: string(page), Icon: icons.Grid, Label: "Schedule controls"},
			Render: func(c *ui.Context, app *native.App) {
				ui.Button(c, button).OnClick(func() {
					source := app.BindAppActions(scope, func() bool { return app.Page == page })
					defer source.Close()
					result = source.Invoke(c, id, json.RawMessage(input))
				})
			},
		})
	}
	register := func(assembly *native.Assembly) {
		if err := assembly.RegisterAppAction(action); err != nil {
			t.Fatal(err)
		}
	}
	open := func(assembly native.Assembly) (*shell.App, func()) {
		app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
		if err := app.RegisterDefaultLinkHandler(shell.LinkRoute{Kind: shell.LinkReference, Name: string(id)}, func(*ui.Context, shell.LinkTarget, shell.LinkActions) error {
			fallback++
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(app.CloseWindowBindings)
		app.Navigate(page)
		window := ui.NewTester(app.View, 960, 640)
		window.Frame()
		return app, func() {
			t.Helper()
			if err := window.Click(button); err != nil {
				t.Fatal(err)
			}
		}
	}
	assert := func(err error, code protocol.ErrorCode, runs int) {
		t.Helper()
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != code || executed != runs || fallback != 0 {
			t.Fatalf("wanted %s, %d executions and no fallback; got %v, %d and %d", code, runs, err, executed, fallback)
		}
	}
	app, click := open(native.Assemble(controls, register))
	click()
	if result != nil || attempts != 1 || executed != 1 || fallback != 0 {
		t.Fatalf("the other module did not execute exactly once: %v, %d/%d/%d", result, attempts, executed, fallback)
	}
	refusal = true
	click()
	assert(result, protocol.ErrorCodePermissionDenied, 1)
	if !errors.Is(result, denied) || attempts != 2 {
		t.Fatal("handler refusal was not final")
	}
	refusal = false
	_, clickMissing := open(native.Assemble(controls))
	clickMissing()
	assert(result, protocol.ErrorCodeUnavailable, 1)
	private := app.BindAppNavigation(scope, func() bool { return true }, nil)
	t.Cleanup(private.Close)
	assert(private.Invoke(nil, id, json.RawMessage(input)), protocol.ErrorCodePermissionDenied, 1)
	assert(private.Invoke(nil, "Fixture.schedule", json.RawMessage(input)), protocol.ErrorCodeInvalidRequest, 1)
	trusted := app.BindAppActions(scope, func() bool { return true })
	t.Cleanup(trusted.Close)
	assert(trusted.Invoke(nil, id, json.RawMessage(`[]`)), protocol.ErrorCodeInvalidRequest, 1)
	if attempts != 2 {
		t.Fatalf("a rejected or unavailable call reached the handler: %d", attempts)
	}
	app.CloseWindowBindings()
	assert(trusted.Invoke(nil, id, json.RawMessage(input)), protocol.ErrorCodeUnavailable, 1)
}

func TestAppActionReleasesExactWindowAndSource(t *testing.T) {
	const id native.AppActionID = "fixture.open"
	app := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	defer app.CloseWindowBindings()
	var calls, replacements int
	var deferred native.AppActionContext
	action := native.AppAction{ID: id, PublicNavigation: true, PluginPermission: protocol.PluginActionReadFiles,
		Run: func(_ *ui.Context, _ *native.App, source native.AppActionContext, _ json.RawMessage) error {
			calls++
			deferred = source
			return nil
		},
	}
	release, err := app.RegisterAppAction(action)
	if err != nil {
		t.Fatal(err)
	}
	other := action
	other.Run = func(*ui.Context, *native.App, native.AppActionContext, json.RawMessage) error {
		replacements++
		return nil
	}
	if conflictingRelease, err := app.RegisterAppAction(other); conflictingRelease != nil || actionFaultCode(err) != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("duplicate registration was not rejected: %v", err)
	}
	permits, active := true, true
	caller := app.BindAppNavigation(native.AppActionScope{Node: protocol.NodeID{1}}, func() bool { return active }, func(permission protocol.PluginAction) bool {
		return permits && permission == protocol.PluginActionReadFiles
	})
	defer caller.Close()
	invoke := func(want protocol.ErrorCode) {
		t.Helper()
		if err := caller.Invoke(nil, id, json.RawMessage(`{}`)); actionFaultCode(err) != want || want == "" && err != nil {
			t.Fatalf("call returned %v instead of %q", err, want)
		}
	}
	invoke("")
	permits = false
	invoke(protocol.ErrorCodePermissionDenied)
	if deferred.Current() {
		t.Fatal("a deferred result survived permission revocation")
	}
	permits, active = true, false
	invoke(protocol.ErrorCodeUnavailable)
	if deferred.Current() {
		t.Fatal("a deferred result survived the source")
	}
	active = true
	release()
	invoke(protocol.ErrorCodeUnavailable)
	if deferred.Current() || calls != 1 || replacements != 0 {
		t.Fatalf("a rejected call executed or retained a deferred result: %d, %d", calls, replacements)
	}
	releaseReplacement, err := app.RegisterAppAction(other)
	if err != nil {
		t.Fatal(err)
	}
	defer releaseReplacement()
	release()
	if deferred.Current() {
		t.Fatal("re-registering an ID revived the previous registration's result")
	}
	invoke("")
	caller.Close()
	invoke(protocol.ErrorCodeUnavailable)
	if calls != 1 || replacements != 1 {
		t.Fatalf("unexpected calls to either registration: %d, %d", calls, replacements)
	}
}

func actionFaultCode(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return ""
}
