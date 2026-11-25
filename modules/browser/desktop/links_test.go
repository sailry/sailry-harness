package desktop

import (
	"errors"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBrowserConversationLinkOverridesSystemDefault(t *testing.T) {
	const address = "https://fixture.invalid/"
	assembly := shell.Assemble(Register)
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	var navigated []string
	available := true
	app.BindPanelResources(assembly, shell.PanelResourceHost{
		BindBrowser: func() {},
		BrowserNavigation: shell.BrowserNavigation{
			Available: func() bool { return available },
			Navigate: func(id, link string) bool {
				if tab := app.Panel.Active; tab.Kind != panel.Browser || tab.ID != id {
					t.Fatalf("navigation did not target the new Browser tab: %+v, %q", tab, id)
				}
				navigated = append(navigated, link)
				return true
			},
		},
	})
	target := shell.LinkTarget{Route: shell.LinkRoute{Kind: shell.LinkScheme, Name: "https"}, URL: address}
	click := true
	tester := ui.NewTester(func(c *ui.Context) {
		if click {
			click = false
			if handled, err := app.DispatchLink(c, target, shell.LinkActions{}); !handled || err != nil {
				t.Fatalf("dispatch returned %v, %v", handled, err)
			}
		}
	}, 320, 240)
	tester.Frame()
	if !slices.Equal(navigated, []string{address}) || len(tester.OpenedURLs()) != 0 {
		t.Fatalf("registered Browser: embedded %v, external %v", navigated, tester.OpenedURLs())
	}
	target.External, click = true, true
	tester.Frame()
	target.External, available, click = false, false, true
	tester.Frame()
	if len(navigated) != 1 || !slices.Equal(tester.OpenedURLs(), []string{address, address}) {
		t.Fatalf("forced external and unavailable Browser: embedded %v, external %v", navigated, tester.OpenedURLs())
	}
}

func TestBrowserConversationLinkReportsWindowRegistrationConflict(t *testing.T) {
	secure := shell.LinkRoute{Kind: shell.LinkScheme, Name: "https"}
	var existing, browser int
	assembly := shell.Assemble(Register, func(a *shell.Assembly) {
		a.RegisterLinkHandler(secure, func(*ui.Context, shell.LinkTarget, shell.LinkActions) error { existing++; return nil })
	})
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	var reports []error
	app.BindPanelResources(assembly, shell.PanelResourceHost{
		BindBrowser: func() {},
		Report:      func(err error) { reports = append(reports, err) },
		BrowserNavigation: shell.BrowserNavigation{
			Available: func() bool { return true },
			Navigate:  func(string, string) bool { browser++; return true },
		},
	})
	var conflict *protocol.Fault
	if len(reports) != 1 || !errors.As(reports[0], &conflict) || conflict.Code != protocol.ErrorCodeRevisionConflict || existing != 0 || browser != 0 {
		t.Fatalf("binding did not report one conflict without invoking a handler: %v, calls %d/%d", reports, existing, browser)
	}
	clicked := false
	tester := ui.NewTester(func(c *ui.Context) {
		if clicked {
			return
		}
		clicked = true
		for _, route := range []shell.LinkRoute{secure, {Kind: shell.LinkScheme, Name: "http"}} {
			if handled, err := app.DispatchLink(c, shell.LinkTarget{Route: route, URL: route.Name + "://fixture.invalid/"}, shell.LinkActions{}); !handled || err != nil {
				t.Fatalf("dispatch %s: %t, %v", route.Name, handled, err)
			}
		}
	}, 320, 240)
	tester.Frame()
	if existing != 1 || browser != 1 || len(tester.OpenedURLs()) != 0 {
		t.Fatalf("the conflict displaced the owner or affected the other scheme: calls %d/%d, external %v", existing, browser, tester.OpenedURLs())
	}
}
