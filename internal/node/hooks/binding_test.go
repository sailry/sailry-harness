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

func TestTurnEndHooks(t *testing.T) {
	settings := protocol.DefaultHooksSettings()
	settings.Enabled = true
	settings.AfterTool = &protocol.HooksScript{Command: "after", Enabled: true, TimeoutMs: 10}
	settings.TurnEnd = &protocol.HooksScript{Command: "end", Enabled: true, TimeoutMs: 10}
	type launch struct{ command, event, status, tool string }
	bind := func(stop context.Context, fail string) (*Binding, *[]launch) {
		var launches []launch
		binding := New(Capture{Settings: settings, Admitted: true, Stop: stop, Claim: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error {
			return nil
		}, Finish: func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error {
			return nil
		}, Launch: func(_ context.Context, command string, _ uint64, env map[string]string) (protocol.ProcessCompletion, error) {
			launches = append(launches, launch{command, env["SAILRY_HOOK_EVENT"], env["SAILRY_HOOK_STATUS"], env["SAILRY_HOOK_TOOL"]})
			code := int32(0)
			if command == fail {
				code = 1
			}
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: code}}, nil
		}})
		return binding, &launches
	}
	t.Run("a finished turn reports completed", func(t *testing.T) {
		// The engine names a successful turn "succeeded"; scripts read the
		// source's word.
		for _, status := range []string{"succeeded", StatusCompleted} {
			binding, launches := bind(context.Background(), "")
			binding.After(context.Background(), "write_file", true)
			if err := binding.Complete(context.Background(), status); err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(*launches, []launch{{"after", "after_tool", "succeeded", "write_file"}, {"end", "turn_end", "completed", ""}}) {
				t.Fatalf("%s: %v", status, *launches)
			}
		}
	})
	t.Run("a failed turn reports failed", func(t *testing.T) {
		binding, launches := bind(context.Background(), "")
		if err := binding.Complete(context.Background(), StatusFailed); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(*launches, []launch{{"end", "turn_end", "failed", ""}}) {
			t.Fatal(*launches)
		}
	})
	t.Run("an After Tool failure fails the turn", func(t *testing.T) {
		binding, launches := bind(context.Background(), "after")
		binding.After(context.Background(), "write_file", false)
		var fault *protocol.Fault
		if err := binding.Complete(context.Background(), StatusCompleted); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(*launches, []launch{{"after", "after_tool", "failed", "write_file"}, {"end", "turn_end", "failed", ""}}) {
			t.Fatal(*launches)
		}
	})
	t.Run("a stopped turn runs no script", func(t *testing.T) {
		stop, cancel := context.WithCancel(context.Background())
		binding, launches := bind(stop, "")
		cancel()
		// The engine completes with a context detached from the stopped turn.
		if err := binding.Complete(context.WithoutCancel(stop), StatusFailed); err != nil {
			t.Fatal(err)
		}
		binding.After(context.WithoutCancel(stop), "write_file", true)
		if len(*launches) != 0 {
			t.Fatal(*launches)
		}
	})
	t.Run("a turn that was not admitted has no binding", func(t *testing.T) {
		if New(Capture{Settings: settings}) != nil {
			t.Fatal("hooks bound a turn that was not admitted")
		}
		var binding *Binding
		if binding.WrapsTools() || binding.Before(context.Background(), "write_file") != nil || binding.Complete(context.Background(), StatusCompleted) != nil {
			t.Fatal("an absent binding ran")
		}
	})
}
