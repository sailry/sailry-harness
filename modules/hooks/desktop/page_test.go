package desktop

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var testCatalog = i18n.New(Locales)

func TestHooksUseManagedNodeWithoutProject(t *testing.T) {
	state := settings.NewLive()
	state.SetNativePages([]settings.NativePage{Page()})
	p := pageFor(state.Model)
	state.Open(settings.Hooks)
	local := settings.HookTarget{Node: "local", Host: "composer_host_local"}
	remote := settings.HookTarget{Node: "remote", Host: "Runner"}
	state.ProviderActions.Hosts = func() []settings.ProviderHost {
		return []settings.ProviderHost{
			{Node: local.Node, Local: true, Connected: true},
			{Node: remote.Node, Name: remote.Host, Connected: true},
		}
	}
	state.HooksContext = func() (settings.HookTarget, bool) {
		if state.ManagedNode == remote.Node {
			return remote, true
		}
		return local, true
	}
	saves := 0
	state.SaveHooks = func(settings.HookSettings) bool { saves++; return true }
	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		theme.Apply(c, "", theme.Dark)
		settings.Page(c, state, nil, settings.Look{})
	}, 940, 700)
	tester.SetLocale("zh-CN")
	tester.Frame()
	label := testCatalog.In("zh-CN", "project_host")
	if _, found := tester.AccessByRole(ui.RoleMenuButton, label); !found {
		t.Fatalf("hooks without a current project has no selector: %q", tester.Texts())
	}
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err := tester.Click(remote.Host); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if state.ManagedNode != remote.Node || !tester.HasText(testCatalog.In("zh-CN", "hooks_empty")) {
		t.Fatalf("the selected host did not show its empty hooks: %q", tester.Texts())
	}
	if err := tester.Click(testCatalog.In("zh-CN", "hooks_add")); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !p.hook.open || p.hook.binding.target != remote {
		t.Fatal("the empty host did not open its own hook editor")
	}
	p.hook.script.Command = "make"
	state.Hooks.Revision++
	tester.Frame()
	closeLabel := testCatalog.In("zh-CN", "close")
	var submit ui.AccessInfo
	for _, element := range tester.AccessAll() {
		if element.Name == closeLabel && element.Role == ui.RoleButton && element.Bounds.Y > submit.Bounds.Y {
			submit = element
		}
	}
	if submit.Bounds.W == 0 {
		t.Fatal("the hook editor has no submit button")
	}
	tester.ClickAt(submit.Bounds.X+submit.Bounds.W/2, submit.Bounds.Y+submit.Bounds.H/2)
	tester.Frame()
	if saves != 0 || !p.hook.open {
		t.Fatal("the stale hook editor submitted or discarded its draft")
	}
}

func TestHookDraftChecks(t *testing.T) {
	t.Run("hook consent reports the captured Node sandbox capability", func(t *testing.T) {
		m := &settings.Model{HooksSandboxed: func() bool { return false }}
		if got := hookScope(m); got != "hooks_scope_workspace" {
			t.Fatalf("preview scope = %q", got)
		}
		m.Live = true
		if got := hookScope(m); got != "hooks_scope_windows" {
			t.Fatalf("Windows scope = %q", got)
		}
		m.HooksSandboxed = func() bool { return true }
		if got := hookScope(m); got != "hooks_scope_workspace" {
			t.Fatalf("sandbox scope = %q", got)
		}
	})
	e := hookEditor{script: preview.Hook{Command: "make"}, timeout: "x"}
	if e.Check() != "hooks_invalid" {
		t.Error("bad timeout accepted")
	}
	e.timeout = "60001"
	if e.Check() != "hooks_invalid" {
		t.Error("oversized hook timeout accepted")
	}
	e.timeout = "500"
	if e.Check() != "" || e.script.TimeoutMS != 500 {
		t.Errorf("good hook refused: %+v", e.script)
	}
}
