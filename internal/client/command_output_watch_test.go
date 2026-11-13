package client

import (
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandOutputOwnerRecovery(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		session, command := protocol.NewSessionID(), protocol.NewRequestID()
		var reads atomic.Int32
		transport := &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
			if reads.Add(1) == 1 {
				return link.Admission{}, fault(protocol.ErrorCodeUnavailable, "fixture command read failed")
			}
			snapshot := protocol.DefaultProcessSnapshot()
			snapshot.Info.ID, snapshot.Info.Session, snapshot.Info.Status.Kind = command, session, "exited"
			snapshot.Stdout.Text = "ready"
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "command_output", Data: snapshot}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}}
		controller := New(transport)
		defer controller.Close()
		type observation struct {
			release func()
			err     error
		}
		replacement := make(chan observation, 1)
		recovered := make(chan struct{}, 1)
		first, err := controller.ObserveCommandOutput(t.Context(), session, command, func() {
			if state, ok := controller.CommandOutput(session, command); !ok || state.Err == nil {
				t.Error("the failed command read was not published to its observer")
			}
			// Re-observe from the failure callback, before the failed owner
			// returns. A deferred cancellation alone is too late.
			release, err := controller.ObserveCommandOutput(t.Context(), session, command, func() { recovered <- struct{}{} })
			replacement <- observation{release, err}
		})
		if err != nil {
			t.Fatal(err)
		}
		defer first()
		synctest.Wait()
		var second observation
		select {
		case second = <-replacement:
		default:
			t.Fatal("the failed command read did not notify its observer")
		}
		if second.err != nil {
			t.Fatal(second.err)
		}
		defer second.release()
		select {
		case <-recovered:
		default:
			t.Fatal("the new observer joined a command output owner that had ended")
		}
		first()
		state, ok := controller.CommandOutput(session, command)
		if reads.Load() != 2 || !ok || !state.Known || state.Err != nil || state.Snapshot.Stdout.Text != "ready" {
			t.Fatalf("the old observer lost the replacement output: reads %d, state %+v, retained %v", reads.Load(), state, ok)
		}
		second.release()
		if _, ok := controller.CommandOutput(session, command); ok {
			t.Fatal("unobserved replacement command output was retained")
		}
	})
}
