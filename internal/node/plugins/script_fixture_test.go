package plugins

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

type scriptFixtureInput struct {
	Entry   string                 `json:"entry"`
	Files   map[string]string      `json:"files"`
	Handler string                 `json:"handler"`
	Input   json.RawMessage        `json:"input"`
	Context protocol.PluginContext `json:"context"`
	Target  protocol.NodeID        `json:"target"`
	Replies []scriptFixtureReply   `json:"replies"`
	Policy  *scriptFixturePolicy   `json:"policy"`
}

type scriptFixtureReply struct {
	Command  protocol.Command           `json:"command"`
	Response map[string]json.RawMessage `json:"response"`
}

type scriptFixturePolicy struct {
	CallID   *protocol.RequestID `json:"call_id"`
	Mode     protocol.WorkMode   `json:"mode"`
	ReadOnly bool                `json:"read_only"`
	Turn     json.RawMessage     `json:"turn"`
}

type scriptFixtureReport struct {
	Result     map[string]any     `json:"result"`
	Calls      []protocol.Request `json:"calls"`
	StagedTurn json.RawMessage    `json:"staged_turn"`
}

type scriptFixtureNoLoader struct{}

func (scriptFixtureNoLoader) Load(string) (any, error) {
	return nil, errors.New("fixture schema retrieval is disabled")
}

func TestMain(suite *testing.M) {
	if os.Getenv("SAILRY_PLUGIN_TEST_HOST") == "" {
		os.Exit(suite.Run())
	}
	encoded, err := io.ReadAll(io.LimitReader(os.Stdin, 8*1024*1024+1))
	var value any
	if err != nil || len(encoded) > 8*1024*1024 {
		err = scriptInvalid("plugin fixture input exceeds limit")
	} else {
		value, err = scriptFixtureRun(encoded, os.Args[1:])
	}
	response := map[string]any{"Ok": value}
	if err != nil {
		response = map[string]any{"Err": scriptFault(err)}
	}
	if json.NewEncoder(os.Stdout).Encode(response) != nil {
		os.Exit(1)
	}
	os.Exit(0)
}

func scriptFixtureRun(encoded []byte, arguments []string) (any, error) {
	if len(arguments) == 1 && arguments[0] == "--validate-schema" {
		var input struct {
			Schema json.RawMessage   `json:"schema"`
			Values []json.RawMessage `json:"values"`
		}
		if protocol.DecodeObject(encoded, &input, []string{"schema", "values"}, true) != nil {
			return nil, scriptInvalid("invalid fixture schema input")
		}
		document, err := scriptDecodeJSON(input.Schema)
		if err != nil {
			return nil, scriptInvalid("invalid or unresolved fixture schema")
		}
		compiler := jsonschema.NewCompiler()
		compiler.UseLoader(scriptFixtureNoLoader{})
		if err := compiler.AddResource("urn:sailry:fixture:schema", document); err != nil {
			return nil, scriptInvalid("invalid or unresolved fixture schema")
		}
		schema, err := compiler.Compile("urn:sailry:fixture:schema")
		if err != nil {
			return nil, scriptInvalid("invalid or unresolved fixture schema")
		}
		valid := make([]bool, len(input.Values))
		for index, encoded := range input.Values {
			value, err := scriptDecodeJSON(encoded)
			valid[index] = err == nil && schema.Validate(value) == nil
		}
		return map[string]any{"valid": valid}, nil
	}
	if len(arguments) == 1 && arguments[0] == "--translation" {
		var input struct {
			Entry   string            `json:"entry"`
			Files   map[string]string `json:"files"`
			Handler string            `json:"handler"`
			Input   json.RawMessage   `json:"input"`
		}
		if protocol.DecodeObject(encoded, &input, []string{"entry", "files", "handler", "input"}, true) != nil {
			return nil, scriptInvalid("invalid translation fixture")
		}
		session, err := NewTranslation(context.Background(), ScriptBundle{Entry: input.Entry, Files: input.Files})
		if err != nil {
			return nil, err
		}
		defer session.Close()
		value, err := session.Call(input.Handler, input.Input, protocol.MAX_FRAME_BYTES)
		return scriptFixtureReport{
			Result: scriptFixtureOutcome(value, err), Calls: []protocol.Request{}, StagedTurn: json.RawMessage("null"),
		}, nil
	}
	if len(arguments) != 0 {
		return nil, scriptInvalid("unsupported plugin test host arguments")
	}
	input := scriptFixtureInput{Target: protocol.NodeID{1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1}}
	if protocol.DecodeObject(encoded, &input, []string{"entry", "files", "handler", "input", "context"}, true) != nil {
		return nil, scriptInvalid("invalid plugin fixture input")
	}
	requests := []protocol.Request{}
	traceBytes := 0
	mismatch := false
	dispatch := func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		encoded, err := json.Marshal(request)
		if err != nil || len(encoded) > 8*1024*1024-traceBytes {
			mismatch = true
			return protocol.Output{}, scriptInvalid("plugin fixture call trace exceeds limit")
		}
		traceBytes += len(encoded)
		requests = append(requests, request)
		if len(input.Replies) == 0 {
			mismatch = true
			return protocol.Output{}, scriptInvalid("plugin fixture received an unexpected request")
		}
		reply := input.Replies[0]
		input.Replies = input.Replies[1:]
		expected, err := scriptJSONValue(reply.Command)
		actual, actualError := scriptJSONValue(request.Command)
		if err != nil || actualError != nil || !reflect.DeepEqual(expected, actual) {
			mismatch = true
			return protocol.Output{}, scriptInvalid("plugin fixture received an unexpected request")
		}
		if encoded, found := reply.Response["Err"]; found {
			var fault protocol.Fault
			if json.Unmarshal(encoded, &fault) != nil {
				mismatch = true
				return protocol.Output{}, scriptInvalid("invalid fixture response")
			}
			return protocol.Output{}, &fault
		}
		var output protocol.Output
		if json.Unmarshal(reply.Response["Ok"], &output) != nil {
			mismatch = true
			return protocol.Output{}, scriptInvalid("invalid fixture response")
		}
		return output, nil
	}
	environment := ScriptEnvironment{Context: input.Context, Target: input.Target, Deadline: time.Now().Add(ScriptCallTimeout)}
	bundle := ScriptBundle{Entry: input.Entry, Files: input.Files}
	var value json.RawMessage
	var err error
	staged := json.RawMessage("null")
	if input.Policy == nil {
		value, err = RunCallback(context.Background(), bundle, input.Handler, input.Input, environment, false, dispatch)
	} else {
		execution := &ScriptExecution{
			CallID: input.Policy.CallID, Mode: input.Policy.Mode, ReadOnly: input.Policy.ReadOnly, Turn: input.Policy.Turn,
		}
		if bytes.Equal(execution.Turn, []byte("null")) {
			execution.Turn = nil
		}
		value, err = RunTool(context.Background(), bundle, input.Handler, input.Input, environment, dispatch, execution)
		if len(execution.StagedTurn) > 0 {
			staged = execution.StagedTurn
		}
	}
	if mismatch || len(input.Replies) != 0 {
		return nil, scriptInvalid("plugin fixture replies did not match all requests")
	}
	return scriptFixtureReport{Result: scriptFixtureOutcome(value, err), Calls: requests, StagedTurn: staged}, nil
}

func scriptFixtureOutcome(value json.RawMessage, err error) map[string]any {
	if err != nil {
		return map[string]any{"Err": scriptFault(err)}
	}
	return map[string]any{"Ok": value}
}
