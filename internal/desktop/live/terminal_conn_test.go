package live

import (
	"bytes"
	"context"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalConnFixture is a connection to a running terminal this desktop
// owns, without a served stream: these checks cover what the view sends.
func terminalConnFixture(t *testing.T, root *Runtime, info protocol.TerminalInfo) *terminalConn {
	t.Helper()
	ctx, cancel := context.WithCancel(root.ctx)
	t.Cleanup(cancel)
	return &terminalConn{owner: root, ctx: ctx, cancel: cancel, ready: make(chan struct{}), info: info, self: root.controller.Target()}
}

func terminalConnQueued(root *Runtime) int {
	root.mu.Lock()
	defer root.mu.Unlock()
	return len(root.queue)
}

func terminalConnInput(t *testing.T, source *terminalTabsTestNode) [][]byte {
	t.Helper()
	var sent [][]byte
	for _, command := range source.sent("input_terminal") {
		payload, valid := detail[protocol.CommandInputTerminalPayload](command.Data)
		if !valid {
			t.Fatalf("unexpected terminal input: %+v", command.Data)
		}
		sent = append(sent, payload.Bytes)
	}
	return sent
}

func TestTerminalControl(t *testing.T) {
	accepted := func(request protocol.Request) link.Response {
		payload, _ := detail[protocol.CommandInputTerminalPayload](request.Command.Data)
		return link.Response{Output: protocol.Output{Kind: "terminal_input", Data: protocol.OutputTerminalInputPayload{Terminal: payload.Terminal}}}
	}
	owned := func(root *Runtime) protocol.TerminalInfo {
		info := terminalTabsTestInfo(1, protocol.WorktreeID{1}, "running")
		self := root.controller.Target()
		info.Owner, info.Revision = &self, 3
		return info
	}
	t.Run("a large paste is sent in order within the Node input bound", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			source.respond = accepted
			connection := terminalConnFixture(t, root, owned(root))
			paste := bytes.Repeat([]byte("0123456789abcdef"), (2*terminalInputLimit+4096)/16)
			if count, err := connection.Write(paste); err != nil || count != len(paste) {
				t.Fatalf("large paste = %d, %v", count, err)
			}
			sent := terminalConnInput(t, source)
			if len(sent) != 3 || !bytes.Equal(bytes.Join(sent, nil), paste) {
				t.Fatalf("large paste was sent as %d commands or changed", len(sent))
			}
			for _, chunk := range sent {
				if len(chunk) > terminalInputLimit {
					t.Fatalf("input command carries %d bytes, over the Node bound", len(chunk))
				}
			}
			if terminalConnQueued(root) != 0 {
				t.Fatal("accepted input reported a failure")
			}
		})
	})
	t.Run("a rejected write keeps input open and is reported once", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			rejecting := true
			source.respond = func(request protocol.Request) link.Response {
				if rejecting {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "terminal ownership changed"}}
				}
				return accepted(request)
			}
			connection := terminalConnFixture(t, root, owned(root))
			for range 2 {
				if count, err := connection.Write([]byte("ls\r")); err != nil || count != 3 {
					t.Fatalf("a rejected write returned %d, %v: MyGo would close the terminal's input", count, err)
				}
			}
			if terminalConnQueued(root) != 1 {
				t.Fatalf("rejected input queued %d reports, want 1", terminalConnQueued(root))
			}
			rejecting = false
			if count, err := connection.Write([]byte("pwd\r")); err != nil || count != 4 {
				t.Fatalf("input after a rejection = %d, %v", count, err)
			}
			if sent := terminalConnInput(t, source); len(sent) != 3 || string(sent[2]) != "pwd\r" {
				t.Fatalf("input did not recover after a rejection: %q", sent)
			}
			rejecting = true
			if _, err := connection.Write([]byte("x")); err != nil || terminalConnQueued(root) != 2 {
				t.Fatalf("a rejection after accepted input was not reported: %v", err)
			}
		})
	})
	t.Run("an oversized write that is rejected drops its rest", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			connection := terminalConnFixture(t, root, owned(root))
			paste := make([]byte, 3*terminalInputLimit)
			if count, err := connection.Write(paste); err != nil || count != len(paste) {
				t.Fatalf("rejected paste = %d, %v", count, err)
			}
			if sent := terminalConnInput(t, source); len(sent) != 1 {
				t.Fatalf("a rejected paste sent %d commands, want 1", len(sent))
			}
		})
	})
	t.Run("input under another controller is dropped without a command", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			info, other := owned(root), protocol.NodeID{9}
			info.Owner = &other
			connection := terminalConnFixture(t, root, info)
			if count, err := connection.Write([]byte("ls\r")); err != nil || count != 3 {
				t.Fatalf("input under another controller = %d, %v", count, err)
			}
			if err := connection.Resize(100, 30); err != nil {
				t.Fatal(err)
			}
			if len(source.sent("input_terminal"))+len(source.sent("resize_terminal")) != 0 || terminalConnQueued(root) != 0 {
				t.Fatal("a terminal another controller owns received commands or a report")
			}
		})
	})
	t.Run("a closed connection ends input", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			source.respond = accepted
			connection := terminalConnFixture(t, root, owned(root))
			connection.cancel()
			if _, err := connection.Write([]byte("ls\r")); err == nil {
				t.Fatal("a closed connection accepted input")
			}
			if len(source.sent("input_terminal")) != 0 {
				t.Fatal("a closed connection sent input")
			}
		})
	})
	t.Run("Take control claims the terminal and restores its size", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			info, other := owned(root), protocol.NodeID{9}
			info.Owner = &other
			source.respond = func(request protocol.Request) link.Response {
				switch request.Command.Kind {
				case "claim_terminal":
					claimed, self := info, root.controller.Target()
					claimed.Owner, claimed.Revision = &self, info.Revision+1
					return link.Response{Output: protocol.Output{Kind: "terminal", Data: claimed}}
				case "resize_terminal":
					claimed, self := info, root.controller.Target()
					claimed.Owner, claimed.Revision = &self, info.Revision+1
					return link.Response{Output: protocol.Output{Kind: "terminal", Data: claimed}}
				}
				return accepted(request)
			}
			connection := terminalConnFixture(t, root, info)
			if err := connection.Resize(100, 30); err != nil {
				t.Fatal(err)
			}
			id := info.ID.String()
			state := &liveTerminal{owner: root, conn: connection}
			root.packages.terminals[id] = state
			root.claimLiveTerminal(id, state)
			root.claimLiveTerminal(id, state)
			if !state.claiming {
				t.Fatal("Take control did not hold while unanswered")
			}
			synctest.Wait()
			pluginViewRunQueue(root)
			claims := source.sent("claim_terminal")
			if len(claims) != 1 {
				t.Fatalf("claim_terminal commands = %d, want 1", len(claims))
			}
			claim, valid := detail[protocol.CommandClaimTerminalPayload](claims[0].Data)
			if !valid || claim.Terminal != info.ID || claim.ExpectedRevision != info.Revision {
				t.Fatalf("unexpected claim: %+v", claim)
			}
			if state.claiming || !connection.controlling() || connection.current().Revision != info.Revision+1 {
				t.Fatal("a granted claim did not make this desktop the controller")
			}
			resizes := source.sent("resize_terminal")
			if len(resizes) != 1 {
				t.Fatalf("resize_terminal commands after a claim = %d, want 1", len(resizes))
			}
			resize, valid := detail[protocol.CommandResizeTerminalPayload](resizes[0].Data)
			if !valid || resize.Viewport.Columns != 100 || resize.Viewport.Rows != 30 || resize.Revision != info.Revision+1 {
				t.Fatalf("unexpected resize after a claim: %+v", resize)
			}
			if count, err := connection.Write([]byte("ls\r")); err != nil || count != 3 || len(source.sent("input_terminal")) != 1 {
				t.Fatal("input was not sent after taking control")
			}
		})
	})
	t.Run("a refused claim leaves the terminal with its controller", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			root, source := terminalTabsFixture(t)
			info, other := owned(root), protocol.NodeID{9}
			info.Owner = &other
			connection := terminalConnFixture(t, root, info)
			if err := connection.Resize(100, 30); err != nil {
				t.Fatal(err)
			}
			id := info.ID.String()
			state := &liveTerminal{owner: root, conn: connection}
			root.packages.terminals[id] = state
			root.claimLiveTerminal(id, state)
			synctest.Wait()
			if len(source.sent("claim_terminal")) != 1 || terminalConnQueued(root) != 1 {
				t.Fatal("a refused claim did not queue its outcome")
			}
			if connection.controlling() || connection.current().Revision != info.Revision {
				t.Fatal("a refused claim took control")
			}
			if len(source.sent("resize_terminal")) != 0 {
				t.Fatal("a refused claim sent the view's size")
			}
		})
	})
	t.Run("Retry attaches a failed terminal again", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			stopped := terminalTabsTestInfo(1, protocol.WorktreeID{1}, "stopped")
			root, source := terminalTabsFixture(t, stopped)
			id := stopped.ID.String()
			state := &liveTerminal{owner: root, failed: true}
			root.packages.terminals[id] = state
			root.retryLiveTerminal(id, state)
			if state.failed || !state.reading {
				t.Fatal("Retry did not start an attach")
			}
			synctest.Wait()
			if len(source.sent("open_terminal")) != 1 {
				t.Fatalf("open_terminal commands = %d, want 1", len(source.sent("open_terminal")))
			}
			root.retryLiveTerminal(id, state)
			synctest.Wait()
			if len(source.sent("open_terminal")) != 1 {
				t.Fatal("Retry attached a terminal that was already attaching or attached")
			}
		})
	})
}
