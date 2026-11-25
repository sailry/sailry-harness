package live

import (
	"context"
	"path/filepath"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	browserdesktop "github.com/sailry/sailry-harness/modules/browser/desktop"
)

type browserAssemblySource struct {
	terminalTabsTestNode
	subscribed []*pluginViewTestSubscription
}

func (s *browserAssemblySource) Subscribe(ctx context.Context, node protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "browser" {
		return s.terminalTabsTestNode.Subscribe(ctx, node, topic)
	}
	stream := &pluginViewTestSubscription{closed: make(chan struct{})}
	s.subscribed = append(s.subscribed, stream)
	return stream, nil
}

func TestBrowserPanelFollowsAssembly(t *testing.T) {
	for _, test := range []struct {
		name     string
		assembly shell.Assembly
		watches  int
	}{
		{name: "without Browser"},
		{name: "with Browser", assembly: shell.Assemble(browserdesktop.Register), watches: 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				snapshot := protocol.DefaultSnapshot()
				snapshot.Node = protocol.NodeID{1}
				source := &browserAssemblySource{}
				source.snapshot = snapshot
				source.respond = func(protocol.Request) link.Response {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
				}
				controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
				t.Cleanup(func() { _ = controller.Close() })
				if err := controller.Start(t.Context()); err != nil {
					t.Fatal(err)
				}
				synctest.Wait()
				r, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), test.assembly)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = r.Close() })
				shown := test.watches != 0
				s := r.App.Panel
				if s.Offers(panel.Browser) != shown || s.ValidTab(panel.Tab{Kind: panel.Browser, ID: "saved"}) != shown {
					t.Fatal("Browser launcher and saved tabs disagree with the assembly")
				}
				r.SetWebViewWindow(new(mygo.Window))
				if r.browser.documents == nil || r.attachments.native.DocumentView == nil || !s.ValidTab(panel.Tab{Kind: panel.Documents}) {
					t.Fatal("the shared document WebView did not survive assembly")
				}
				if (s.BrowserActions != nil) != shown {
					t.Fatal("Browser actions disagree with the assembly")
				}
				if !shown {
					s.Open(panel.Browser)
					s.New(panel.Browser)
					if tab := s.AddBrowser(false); tab != (panel.Tab{}) || len(s.Tabs) != 0 {
						t.Fatalf("unregistered Browser opened a tab: %v", s.Tabs)
					}
				}
				for range 3 {
					r.frameNativeWindowResources()
					synctest.Wait()
				}
				if count := len(source.subscribed); count != test.watches {
					t.Fatalf("the native window opened %d browser subscriptions, want %d", count, test.watches)
				}
				if err := r.Close(); err != nil {
					t.Fatal(err)
				}
				if shown {
					select {
					case <-source.subscribed[0].closed:
					default:
						t.Fatal("the browser subscription survived window close")
					}
				}
			})
		})
	}
}

func TestBrowserPanelWithoutModuleOpensExternalLinks(t *testing.T) {
	const address = "https://fixture.invalid/"
	r := &Runtime{
		App:     shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		browser: &browserState{available: true},
	}
	tester := ui.NewTester(func(c *ui.Context) { r.openChatLink(c, "", address) }, 320, 240)
	tester.Frame()
	if opened := tester.OpenedURLs(); len(opened) == 0 || opened[0] != address || len(r.App.Panel.Tabs) != 0 {
		t.Fatalf("without Browser a web link must open externally: %v, tabs %v", opened, r.App.Panel.Tabs)
	}
}
