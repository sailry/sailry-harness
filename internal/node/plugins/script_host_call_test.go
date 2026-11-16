package plugins

import (
	"context"
	"encoding/json"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostCallPreservesExecutionNodeAndTarget(t *testing.T) {
	var requests []protocol.Request
	source := `import {context,prepareHostCall,completeRequest} from 'sailry/sdk';
export async function run() {
  const scope=context(), selected=Array(32).fill(0); selected[0]=9;
  const result=await completeRequest(prepareHostCall(selected,'save',{value:'kept'}));
  return {node:scope.execution_node,result};
}`
	value, err := scriptTestRun(t, source, func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		requests = append(requests, request)
		return protocol.Output{Kind: "plugin_result", Data: json.RawMessage(`{"Ok":true}`)}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	var result struct {
		Node protocol.NodeID `json:"node"`
	}
	if err := json.Unmarshal(value, &result); err != nil || result.Node != (protocol.NodeID{1}) || len(requests) != 1 {
		t.Fatalf("execution identity: %s %+v %v", value, requests, err)
	}
	request := requests[0]
	payload, ok := protocol.HostHandlerPayload(request.Command)
	if !ok || request.Target != (protocol.NodeID{9}) || request.Plugin == nil ||
		payload.Handler != "save" || !reflect.DeepEqual(payload.Input, map[string]any{"value": "kept"}) {
		t.Fatalf("host request lost scope or target: %+v", request)
	}
}

func TestReadOnlyHostCallRejectsMutations(t *testing.T) {
	source := `import {context,prepareHostCall,completeRequest} from 'sailry/sdk';
export async function run() {return await completeRequest(prepareHostCall(context().execution_node,'save',{},false));}`
	_, err := RunCallback(t.Context(), scriptTestBundle(source), "run", nil, scriptTestEnvironment(), true,
		func(context.Context, protocol.Request) (protocol.Output, error) {
			t.Fatal("read-only callback dispatched mutation")
			return protocol.Output{}, nil
		})
	scriptTestFault(t, err, protocol.ErrorCodePermissionDenied)
}
