package live

import (
	"slices"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceTransferInspectsOriginalAfterCancellingRemaining(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t)
		root := fixture.root
		project := root.workspaceFor(fixture.main)
		applied := make(chan link.Response, 1)
		var requestID protocol.RequestID
		out := protocol.Output{Kind: "entry_moved", Data: protocol.OutputEntryMovedPayload{From: "one.txt", To: "folder/one.txt"}}
		fixture.source.outcome = protocol.RequestOutcome{Kind: "admitted"}
		fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
			switch request.Command.Kind {
			case "move_entry_to":
				requestID = request.ID
				fixture.source.prepared[request.ID] = request
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: applied}, nil
			default:
				return workspaceFilesAdmission(request, protocol.Output{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "unexpected transfer command"))
			}
		}
		var run func(*ui.Context)
		window := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			root.mu.Lock()
			queue := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, complete := range queue {
				complete(c)
			}
			if run != nil {
				action := run
				run = nil
				action(c)
			}
		}, 560, 380)
		step := func(action func(*ui.Context)) {
			run = action
			window.Frame()
			synctest.Wait()
			window.Frame()
		}
		step(func(c *ui.Context) {
			project.fileAction(c, panel.FileAction{Kind: "files_cut", Paths: []string{"one.txt", "two.txt"}})
			project.fileAction(c, panel.FileAction{Kind: "files_paste", Directory: "folder"})
		})
		transfer := root.workspace.transfer
		project.panel.SetFile("one.txt", "original")
		project.panel.SetFileWritable("one.txt", true)
		project.panel.FileChanged("one.txt", "unsaved draft")
		step(func(c *ui.Context) { transfer.start(c, "folder/one.txt") })
		if draft, dirty := project.panel.FileDraft("one.txt"); !dirty || draft != "unsaved draft" || len(fixture.source.commandKinds()) != 0 {
			t.Fatalf("moving an unsaved file discarded the draft or sent a request: %q, %v", draft, fixture.source.commandKinds())
		}
		project.panel.DropFileDraft("one.txt")
		step(func(c *ui.Context) { transfer.start(c, "folder/one.txt") })
		step(transfer.cancel)
		// Dropping the reply simulates a request still admitted while the user
		// closes the transfer. The next operation must inspect that request.
		close(applied)
		step(nil)
		if transfer.view.Status != transfers.EntryUncertain || transfer.view.Open || transfer.index != 0 || !transfer.stop {
			t.Fatalf("cancellation replaced the admitted request: %+v", transfer.view)
		}
		fixture.source.outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &out}}
		step(func(*ui.Context) { transfer.check() })
		if requestID == (protocol.RequestID{}) || transfer.request.ID != requestID || transfer.view.Status != transfers.EntryCancelled || transfer.index != 1 {
			t.Fatalf("the captured outcome was not settled: request %v, transfer %+v", requestID, transfer.view)
		}
		if got := fixture.source.commandKinds(); !slices.Equal(got, []string{"move_entry_to", "inspect_request", "inspect_request"}) {
			t.Fatalf("checking or cancelling dispatched a different operation: %v", got)
		}
		if got := root.workspace.clipboard.paths; !slices.Equal(got, []string{"two.txt"}) || root.workspace.transfer != nil || len(root.workspace.transfers) != 0 {
			t.Fatalf("cancelled batch retained the moved source or retired transfer: %q", got)
		}
	})
}
