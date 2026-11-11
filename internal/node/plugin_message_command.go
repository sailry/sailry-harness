package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (node *Node) dispatchPluginMessageCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, bool, error) {
	if !pluginMessageCommandCandidate(request) {
		return link.Admission{}, false, nil
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, true, err
	}
	var child *protocol.Request
	var previous *link.Response
	var resolution error
	handled := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		outcome, err := admission.Inspect(ctx, db, caller, request.ID, fmt.Sprintf("%x", blake3.Sum256(body)))
		if err != nil {
			return err
		}
		if outcome.Kind != "not_admitted" {
			handled = true
			previous, err = admission.Admit(ctx, db, caller, request, body)
			return err
		}
		child, resolution = store.ResolvePluginMessageCommand(ctx, db, node.ID(), request)
		handled = child != nil || resolution != nil
		if !handled {
			return nil
		}
		previous, err = admission.Admit(ctx, db, caller, request, body)
		if err != nil || previous != nil {
			return err
		}
		if resolution != nil {
			return admission.Complete(ctx, db, caller, request.ID, response(protocol.Output{}, resolution))
		}
		return nil
	})
	if err != nil || !handled {
		node.work.Done()
		return link.Admission{}, handled || err != nil, nodeFault(err)
	}
	completed := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: completed}
	if previous != nil || resolution != nil {
		node.work.Done()
		result := response(protocol.Output{}, resolution)
		if previous != nil {
			result = *previous
		}
		completed <- result
		close(completed)
		return accepted, true, nil
	}
	work := func(owner context.Context) {
		defer node.work.Done()
		defer close(completed)
		output, err := node.runPluginMessageCommand(owner, caller, *child)
		completed <- node.settle(caller, request.ID, response(output, err))
	}
	if err := node.tasks.Submit(work); err != nil {
		completed <- node.settle(caller, request.ID, response(protocol.Output{}, err))
		close(completed)
		node.work.Done()
	}
	return accepted, true, nil
}

func pluginMessageCommandCandidate(request protocol.Request) bool {
	if request.Plugin != nil {
		return false
	}
	var message protocol.ConversationInput
	switch request.Command.Kind {
	case "submit_turn":
		input, valid := dispatchPayload[protocol.CommandSubmitTurnPayload](request.Command.Data)
		if !valid {
			return false
		}
		message = input.Message
	case "submit_task":
		input, valid := dispatchPayload[protocol.CommandSubmitTaskPayload](request.Command.Data)
		if !valid {
			return false
		}
		message = input.Message
	default:
		return false
	}
	return strings.HasPrefix(strings.TrimLeftFunc(message.Text, unicode.IsSpace), "/")
}

func (node *Node) runPluginMessageCommand(ctx context.Context, caller protocol.NodeID, child protocol.Request) (protocol.Output, error) {
	accepted, err := node.Dispatch(ctx, caller, child)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case <-ctx.Done():
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "slash command completion is unknown"}
	case result, open := <-accepted.Completion:
		if !open {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "slash command completion is unknown"}
		}
		if result.Fault != nil {
			return protocol.Output{}, result.Fault
		}
		return pluginMessageCommandOutput(child, result.Output)
	}
}

func pluginMessageCommandOutput(child protocol.Request, output protocol.Output) (protocol.Output, error) {
	if output.Kind != "plugin_result" {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "invalid command callback output"}
	}
	value, captured := dispatchPayload[json.RawMessage](output.Data)
	if !captured {
		var err error
		value, err = json.Marshal(output.Data)
		if err != nil {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid command callback output"}
		}
	}
	var header struct {
		Kind string `json:"kind"`
	}
	if json.Unmarshal(value, &header) != nil || header.Kind != "queued_turn" {
		return output, nil
	}
	var decoded protocol.Output
	if json.Unmarshal(value, &decoded) != nil || decoded.Kind != "queued_turn" {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid command turn output"}
	}
	turn, valid := dispatchPayload[protocol.QueuedTurn](decoded.Data)
	if !valid {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid command turn output"}
	}
	if child.Plugin == nil || child.Plugin.Session == nil || turn.Session != *child.Plugin.Session {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "command turn belongs to another session"}
	}
	return protocol.Output{Kind: "queued_turn", Data: turn}, nil
}
