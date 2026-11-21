//go:build integration && (darwin || linux)

package node

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOrdinarySessionNativeCommandRoundTrip(t *testing.T) {
	t.Parallel()
	script := &agentToolScript{}
	node, config := ordinarySessionCommandFixture(t, script, process.Options{})
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	firstRequest, ready := make(chan struct{}), make(chan struct{})
	script.reset(func(step int, results map[string]string) [][3]string {
		switch step {
		case 1:
			close(firstRequest)
			select {
			case <-ready:
				return [][3]string{
					{"root", "run_command", `{"command":"pwd -P","timeout_ms":5000}`},
					{"relative", "run_command", `{"command":"pwd -P","cwd":"subdir","timeout_ms":5000}`},
				}
			case <-ctx.Done():
				return nil
			}
		case 2:
			var calls [][3]string
			for _, id := range []string{"root", "relative"} {
				if call, ok := ordinaryCommandReadCall("read-"+id, results[id]); ok {
					calls = append(calls, call)
				}
			}
			return calls
		}
		return nil
	})
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	start := protocol.DefaultConversationStart()
	start.Config, start.Execution, start.UseExplicitConfig = &config, protocol.ConversationExecutionModel, true
	start.Message = protocol.ConversationInput{Text: "Report the working directory and read it back"}
	turn := commandFixture(t, node, protocol.Command{Kind: "start_session", Data: start}).Data.(protocol.QueuedTurn)
	if turn.Config.Mode != config.Mode || turn.Config.Permission != config.Permission {
		t.Fatalf("admitted configuration = %+v", turn.Config)
	}
	session := ordinarySessionStarted(t, ctx, subscription, turn)
	select {
	case <-firstRequest:
	case <-ctx.Done():
		t.Fatalf("no model request for %s: %v", turn.ID, ctx.Err())
	}
	root, err := filepath.EvalSymlinks(core.SessionWorkspace(node.profile.Ownership.Path, session.ID))
	if err != nil {
		t.Fatal(err)
	}
	subdir := filepath.Join(root, "subdir")
	if err := os.Mkdir(subdir, 0o700); err != nil {
		t.Fatal(err)
	}
	close(ready)
	for {
		var run protocol.ConversationRun
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			run, err = store.ReadAgentRun(ctx, db, turn.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if run.Status == protocol.ConversationStatusCompleted {
			break
		}
		switch run.Status {
		case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
			t.Fatalf("native command turn ended with %s: %v", run.Status, run.Error)
		}
		if _, err := subscription.Next(ctx); err != nil {
			t.Fatalf("waiting for native command turn %s in state %s: %v", turn.ID, run.Status, err)
		}
	}
	script.mu.Lock()
	defer script.mu.Unlock()
	rootID := expectOrdinaryCommandReadback(t, script.results["root"], script.results["read-root"], root+"\n")
	relativeID := expectOrdinaryCommandReadback(t, script.results["relative"], script.results["read-relative"], subdir+"\n")
	if rootID == relativeID {
		t.Fatalf("native commands shared process ID %s", rootID)
	}
}
