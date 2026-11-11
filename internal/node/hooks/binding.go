package hooks

import (
	"context"
	"errors"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Capture contains immutable admission data and callbacks into Node ownership.
// Claim and Finish must commit through the existing storage writer. Launch must
// run outside that transaction and retain its own native process ownership.
type Capture struct {
	Node     protocol.NodeID
	Worktree protocol.WorktreeID
	Session  protocol.SessionID
	Turn     protocol.TurnID
	Settings protocol.HooksSettings
	Admitted bool
	Stop     context.Context
	Claim    func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64) error
	Finish   func(context.Context, protocol.TurnID, protocol.HooksPhase, uint64, protocol.ProcessOutcome, error) error
	Launch   func(context.Context, string, uint64, map[string]string) (protocol.ProcessCompletion, error)
}

type Binding struct {
	captured Capture
	before   atomic.Uint64
	after    atomic.Uint64
	mu       sync.Mutex
	failure  error
}

// New freezes every script value so later settings updates affect new turns only.
func New(input Capture) *Binding {
	if !input.Admitted || !input.Settings.Enabled {
		return nil
	}
	settings := input.Settings
	clone := func(script *protocol.HooksScript) *protocol.HooksScript {
		if script == nil {
			return nil
		}
		value := *script
		return &value
	}
	settings.BeforeTool, settings.AfterTool, settings.TurnEnd = clone(settings.BeforeTool), clone(settings.AfterTool), clone(settings.TurnEnd)
	settings.Additional = append([]protocol.HooksEntry(nil), settings.Additional...)
	input.Settings = settings
	if input.Stop == nil {
		input.Stop = context.Background()
	}
	return &Binding{captured: input}
}

func (b *Binding) WrapsTools() bool {
	return b != nil && (len(Scripts(b.captured.Settings, protocol.HooksPhaseBeforeTool)) > 0 || len(Scripts(b.captured.Settings, protocol.HooksPhaseAfterTool)) > 0)
}
func (b *Binding) Before(ctx context.Context, tool string) error {
	if b == nil {
		return nil
	}
	return b.execute(ctx, protocol.HooksPhaseBeforeTool, b.before.Add(1)-1, tool, "")
}
func (b *Binding) After(ctx context.Context, tool string, success bool) {
	if b == nil {
		return
	}
	status := "failed"
	if success {
		status = "succeeded"
	}
	if err := b.execute(ctx, protocol.HooksPhaseAfterTool, b.after.Add(1)-1, tool, status); err != nil {
		b.remember(err)
	}
}

// Turn End scripts read one of these two words in SAILRY_HOOK_STATUS.
const (
	StatusCompleted = "completed"
	StatusFailed    = "failed"
)

// Complete runs the Turn End scripts. Anything but StatusFailed is a completed
// turn, and an earlier After Tool failure makes the turn failed. A stopped
// turn (the captured Stop is done) runs no script.
func (b *Binding) Complete(ctx context.Context, status string) error {
	if b == nil {
		return nil
	}
	if status != StatusFailed {
		status = StatusCompleted
	}
	b.mu.Lock()
	if b.failure != nil {
		status = StatusFailed
	}
	b.mu.Unlock()
	if err := b.execute(ctx, protocol.HooksPhaseTurnEnd, 0, "", status); err != nil {
		b.remember(err)
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.failure
}
func (b *Binding) remember(err error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.failure == nil {
		b.failure = err
		return
	}
	code := protocol.ErrorCodeUnavailable
	var previous, current *protocol.Fault
	if errors.As(b.failure, &previous) {
		code = previous.Code
	}
	if errors.As(err, &current) && current.Code == protocol.ErrorCodeOutcomeUnknown {
		code = current.Code
	}
	message := hookMessage(b.failure)
	next := hookMessage(err)
	if len(message) < 4096 && !strings.Contains(message, next) {
		message += "; "
		chars := []rune(next)
		message += string(chars[:min(len(chars), max(4096-len(message), 0))])
	}
	b.failure = &protocol.Fault{Code: code, Message: message}
}
func hookMessage(err error) string {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Message
	}
	return err.Error()
}
func (b *Binding) execute(ctx context.Context, phase protocol.HooksPhase, ordinal uint64, tool, status string) (executionErr error) {
	// The hook boundary retains the source's uncertain outcome after unwinding.
	defer func() {
		if recover() != nil {
			executionErr = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "hook execution failed; side effects were not replayed"}
		}
	}()
	scripts := Scripts(b.captured.Settings, phase)
	if len(scripts) == 0 || b.captured.Stop.Err() != nil {
		return nil
	}
	if b.captured.Claim == nil || b.captured.Finish == nil || b.captured.Launch == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "hook execution owner is unavailable"}
	}
	if err := b.captured.Claim(ctx, b.captured.Turn, phase, ordinal); err != nil {
		return err
	}
	metadata := map[string]string{
		"SAILRY_HOOK_EVENT": string(phase), "SAILRY_HOOK_NODE": b.captured.Node.String(),
		"SAILRY_HOOK_WORKTREE": b.captured.Worktree.String(), "SAILRY_HOOK_SESSION": b.captured.Session.String(), "SAILRY_HOOK_TURN": b.captured.Turn.String(),
	}
	if tool != "" {
		metadata["SAILRY_HOOK_TOOL"] = tool
	}
	if status != "" {
		metadata["SAILRY_HOOK_STATUS"] = status
	}
	outcome := protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}
	var result error
	for _, entry := range scripts {
		if ctx.Err() != nil || b.captured.Stop.Err() != nil {
			if success(outcome, result) {
				outcome = protocol.ProcessOutcome{Kind: "cancelled"}
			}
			break
		}
		completion, err := b.captured.Launch(ctx, entry.Script.Command, entry.Script.TimeoutMs, metadata)
		if !success(completion.Outcome, err) {
			uncertain := isUnknown(err) || completion.Outcome.Kind == "unknown"
			if success(outcome, result) || uncertain {
				outcome, result = completion.Outcome, err
			}
			if phase == protocol.HooksPhaseBeforeTool || uncertain {
				break
			}
		}
	}
	// Completion recording is not cancelled with the effect; an interrupted script
	// must leave a terminal receipt instead of looking safe to execute again.
	if err := b.captured.Finish(context.WithoutCancel(ctx), b.captured.Turn, phase, ordinal, outcome, result); err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "hook completion could not be recorded: " + err.Error() + "; script was not replayed"}
	}
	if result != nil {
		return result
	}
	if success(outcome, nil) {
		return nil
	}
	code := protocol.ErrorCodeUnavailable
	if phase == protocol.HooksPhaseBeforeTool {
		code = protocol.ErrorCodePermissionDenied
	}
	if outcome.Kind == "unknown" {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: string(phase) + " hook outcome is uncertain" + outcomeDetail(outcome) + "; script was not replayed"}
	}
	return &protocol.Fault{Code: code, Message: string(phase) + " hook failed: " + outcome.Kind + outcomeDetail(outcome)}
}
func success(outcome protocol.ProcessOutcome, err error) bool {
	if err != nil || outcome.Kind != "exited" {
		return false
	}
	switch code := outcome.Data.(type) {
	case int32:
		return code == 0
	case *int32:
		return code != nil && *code == 0
	case int:
		return code == 0
	}
	return false
}
func isUnknown(err error) bool {
	var value *protocol.Fault
	return errors.As(err, &value) && value.Code == protocol.ErrorCodeOutcomeUnknown
}
func outcomeDetail(outcome protocol.ProcessOutcome) string {
	switch value := outcome.Data.(type) {
	case int32:
		return " " + strconv.FormatInt(int64(value), 10)
	case *int32:
		if value != nil {
			return " " + strconv.FormatInt(int64(*value), 10)
		}
	case string:
		return ": " + value
	case *string:
		if value != nil {
			return ": " + *value
		}
	}
	return ""
}
