package node

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) subscribeScopedInstallations(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	request, valid := dispatchPayload[protocol.Request](topic.Data)
	if !valid {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "installation observation expected"}
	}
	body, err := json.Marshal(request)
	if err != nil || json.Unmarshal(body, &request) != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid installation observation"}
	}
	if request.Target != node.ID() || request.Version != protocol.VERSION {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "Node or protocol version mismatch"}
	}
	input, valid := dispatchPayload[protocol.CommandReadInstallationsPayload](request.Command.Data)
	if request.Command.Kind != "read_installations" || !valid {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "installation observation expected"}
	}
	if request.Plugin == nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin scope expected"}
	}
	if err := node.begin(); err != nil {
		return nil, err
	}
	defer node.work.Done()
	if err := node.validatePluginRequest(ctx, caller, request); err != nil {
		return nil, nodeFault(err)
	}
	return node.plugins.installations.SubscribeFor(caller, input.Requests, *request.Plugin)
}
