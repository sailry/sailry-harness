package hooks

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHooksCapturedAtAdmission(t *testing.T) {
	settings := protocol.DefaultHooksSettings()
	settings.Enabled = true
	settings.BeforeTool = &protocol.HooksScript{Command: "captured", Enabled: true, TimeoutMs: 30000}
	settings.Additional = []protocol.HooksEntry{{ID: "second", Phase: protocol.HooksPhaseBeforeTool, Script: protocol.HooksScript{Command: "second", Enabled: true, TimeoutMs: 100}}}
	var calls []string
	binding := New(Capture{Settings: settings, Admitted: true, Claim: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error {
		calls = append(calls, "claim")
		return nil
	}, Finish: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error {
		calls = append(calls, "finish")
		return nil
	}, Launch: func(_ context.Context, command string, timeout uint64, env map[string]string) (protocol.ProcessCompletion, error) {
		calls = append(calls, command)
		if env["SAILRY_HOOK_TOOL"] != "read_file" {
			t.Fatal(env)
		}
		if command == "captured" && timeout != 30000 {
			t.Fatal(timeout)
		}
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	}})
	settings.BeforeTool.Command = "changed"
	settings.Additional[0].Script.Command = "changed too"
	if err := binding.Before(context.Background(), "read_file"); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(calls, []string{"claim", "captured", "second", "finish"}) {
		t.Fatal(calls)
	}
}

func TestUncertainHookClaimNeverLaunches(t *testing.T) {
	settings := protocol.DefaultHooksSettings()
	settings.Enabled = true
	settings.BeforeTool = &protocol.HooksScript{Command: "effect", Enabled: true, TimeoutMs: 10}
	launched := false
	binding := New(Capture{Settings: settings, Admitted: true, Claim: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "already admitted"}
	}, Finish: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error {
		return nil
	}, Launch: func(context.Context, string, uint64, map[string]string) (protocol.ProcessCompletion, error) {
		launched = true
		return protocol.ProcessCompletion{}, nil
	}})
	var fault *protocol.Fault
	if err := binding.Before(context.Background(), "write_file"); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeOutcomeUnknown || launched {
		t.Fatal(err, launched)
	}
}
