package engine

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func validateInstructionRecord(record canonical.Record) error {
	encoded, err := json.Marshal(record)
	if err != nil {
		return err
	}
	if len(encoded) > protocol.MAX_FRAME_BYTES {
		context := record.Data.(*canonical.Configuration).Context
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("captured instructions (%s) encode to %d bytes, exceeding the %d-byte conversation history record capacity; reduce the instructions before retrying", instructionSources(context), len(encoded), protocol.MAX_FRAME_BYTES)}
	}
	return nil
}

func validateInstructionContext(binding model.Binding, context canonical.Context, child bool) error {
	if len(context.Guides) == 0 && (context.Personal == nil || *context.Personal == "") {
		return nil
	}
	// Stable instructions are not summarized. Reject only when they alone fill
	// the model window; this estimate does not impose a new history/tool budget.
	estimated := tokens.Text(prompt.System(prompt.Instruction(child), context)) + 4
	limit := uint64(model.ContextLimit(binding))
	if estimated >= limit {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("instructions from %s and the fixed system prompt require an estimated %d tokens, filling model %q's %d-token context window; reduce the fixed instructions or choose a model with a larger context window", instructionSources(context), estimated, binding.Config.Model, limit)}
	}
	return nil
}

func instructionSources(context canonical.Context) string {
	paths := make([]string, 0, len(context.Guides)+1)
	if context.Personal != nil && *context.Personal != "" {
		paths = append(paths, "instructions.md")
	}
	for _, guide := range context.Guides {
		paths = append(paths, guide.Path)
	}
	if len(paths) == 0 {
		return "the fixed system prompt"
	}
	slices.Sort(paths)
	return strings.Join(paths, ", ")
}
