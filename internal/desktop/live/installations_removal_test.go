package live

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testInstallationRemoval removes a failed installation whose Node refuses
// the removal: the row stays and the failure toast names the removal, not a
// plugin operation. Once the Node accepts, the row leaves without a toast.
// It runs as a step of TestComposerSendRouting.
func testInstallationRemoval(t *testing.T) {
	request := protocol.RequestID{7}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	transport := &conversationTestTransport{snapshot: snapshot}
	refuse, sent := true, 0
	transport.handle = func(r protocol.Request) (link.Response, bool) {
		if r.Command.Kind != "dismiss_installation" {
			return link.Response{}, false
		}
		sent++
		if refuse {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "the queue is busy"}}, true
		}
		return link.Response{}, true
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	tester := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	settle := func() {
		for range 4 {
			r.workers.Wait()
			tester.Advance(100 * time.Millisecond)
			tester.Frame()
		}
	}
	settle()
	// What the fixture Node's empty answers toasted at startup is gone.
	tester.Advance(time.Minute)
	tester.Frame()
	if tester.HasText(i18n.In("en", "plugins_failed")) {
		t.Fatalf("a startup toast stayed: %q", tester.Texts())
	}
	r.packages.tasks = []protocol.InstallationTask{{Request: request}}
	entry := installationEntry(snapshot.Node.String(), r.packages.tasks[0], true)
	failed, generic := i18n.In("en", "installation_remove_failed"), i18n.In("en", "plugins_failed")
	r.dismissInstallation(entry)
	settle()
	if sent != 1 || len(r.packages.tasks) != 1 || !tester.HasText(failed) || tester.HasText(generic) {
		t.Fatalf("refused removal: %d sent, %d tasks, %q", sent, len(r.packages.tasks), tester.Texts())
	}
	refuse = false
	tester.Advance(time.Minute)
	tester.Frame()
	r.dismissInstallation(entry)
	settle()
	if sent != 2 || len(r.packages.tasks) != 0 || tester.HasText(failed) {
		t.Fatalf("accepted removal: %d sent, %d tasks, %q", sent, len(r.packages.tasks), tester.Texts())
	}
}
