package node

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"os/exec"
	"slices"
	"strings"
	"time"

	"github.com/kaptinlin/jsonpointer"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type connectorOutput struct {
	bytes.Buffer
	limit int
}

func (output *connectorOutput) Write(body []byte) (int, error) {
	if len(body) > output.limit-output.Len() {
		return 0, io.ErrShortBuffer
	}
	return output.Buffer.Write(body)
}

func (node *Node) connectorCLIIdentity(ctx context.Context, declaration protocol.PluginConnectorDeclaration) (protocol.PluginConnectorIdentity, error) {
	auth, err := connectorAuth(declaration, protocol.PluginConnectorKindLocalCli)
	if err != nil {
		return protocol.PluginConnectorIdentity{}, err
	}
	definition, ok := dispatchPayload[protocol.PluginConnectorAuthLocalCliPayload](auth.Data)
	if !ok {
		return protocol.PluginConnectorIdentity{}, connectorUnavailable()
	}
	body, err := node.runConnectorCLI(ctx, definition, definition.Status.Argv)
	if err != nil {
		return protocol.PluginConnectorIdentity{}, err
	}
	var document any
	if json.Unmarshal(body, &document) != nil {
		return protocol.PluginConnectorIdentity{}, connectorUnavailable()
	}
	if condition := definition.Status.Condition; condition != nil {
		value, err := jsonpointer.Value(document, condition.Pointer)
		if err != nil {
			return protocol.PluginConnectorIdentity{}, connectorUnavailable()
		}
		actual, err := json.Marshal(value)
		if err != nil || !slices.ContainsFunc(condition.Values, func(expected json.RawMessage) bool { return bytes.Equal(bytes.TrimSpace(expected), actual) }) {
			return protocol.PluginConnectorIdentity{}, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "local account is signed out"}
		}
	}
	value, err := jsonpointer.Value(document, definition.Status.Account)
	account, ok := value.(string)
	if err != nil || !ok || account == "" || len(account) > 128 || strings.ContainsFunc(account, func(character rune) bool {
		return !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || strings.ContainsRune("._@-", character))
	}) {
		return protocol.PluginConnectorIdentity{}, connectorUnavailable()
	}
	return protocol.PluginConnectorIdentity{Name: account, ID: account}, nil
}

func (node *Node) connectorCLIToken(ctx context.Context, declaration protocol.PluginConnectorDeclaration, expected *protocol.PluginConnectorIdentity) (protocol.Secret, error) {
	auth, err := connectorAuth(declaration, protocol.PluginConnectorKindLocalCli)
	if err != nil || expected == nil {
		return protocol.Secret{}, connectorUnavailable()
	}
	definition, ok := dispatchPayload[protocol.PluginConnectorAuthLocalCliPayload](auth.Data)
	if !ok {
		return protocol.Secret{}, connectorUnavailable()
	}
	before, err := node.connectorCLIIdentity(ctx, declaration)
	if err != nil {
		return protocol.Secret{}, err
	}
	if before.ID != expected.ID {
		return protocol.Secret{}, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "local account changed; connect again"}
	}
	arguments := slices.Clone(definition.Token.Argv)
	for index := range arguments {
		arguments[index] = strings.ReplaceAll(arguments[index], "{account}", before.ID)
	}
	body, err := node.runConnectorCLI(ctx, definition, arguments)
	if err != nil {
		return protocol.Secret{}, err
	}
	token := ""
	if definition.Token.Parse.Kind == "line" {
		for _, line := range strings.Split(string(body), "\n") {
			if value := strings.TrimSpace(line); value != "" {
				token = value
				break
			}
		}
	} else if path, ok := dispatchPayload[string](definition.Token.Parse.Data); ok {
		var document any
		if json.Unmarshal(body, &document) == nil {
			value, err := jsonpointer.Value(document, path)
			if err == nil {
				token, _ = value.(string)
			}
		}
	}
	if token == "" || strings.ContainsAny(token, "\r\n\x00") {
		return protocol.Secret{}, connectorUnavailable()
	}
	after, err := node.connectorCLIIdentity(ctx, declaration)
	if err != nil {
		return protocol.Secret{}, err
	}
	if before.ID != after.ID {
		return protocol.Secret{}, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "local account changed; connect again"}
	}
	return protocol.NewSecret(token), nil
}

func (node *Node) runConnectorCLI(ctx context.Context, declaration protocol.PluginConnectorAuthLocalCliPayload, arguments []string) ([]byte, error) {
	if declaration.TimeoutMs == 0 || declaration.TimeoutMs > 60_000 || declaration.MaxOutput == 0 || declaration.MaxOutput > 1024*1024 {
		return nil, connectorUnavailable()
	}
	// The search path and account directories come from the Node alone: a
	// package's declaration must not choose the executable or its home.
	owned := []string{"PATH", "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "SystemRoot", "SystemDrive", "PATHEXT"}
	environment := map[string]string{}
	for name, value := range declaration.Env {
		if !slices.ContainsFunc(owned, func(reserved string) bool { return strings.EqualFold(reserved, name) }) {
			environment[name] = value
		}
	}
	for _, entry := range node.environment.TerminalValues() {
		name, value, _ := strings.Cut(entry, "=")
		if slices.Contains(owned, name) {
			environment[name] = value
		}
	}
	executable, err := program.Resolve(declaration.Command, environment)
	if err != nil {
		return nil, connectorUnavailable()
	}
	operation, cancel := context.WithTimeout(ctx, time.Duration(declaration.TimeoutMs)*time.Millisecond)
	defer cancel()
	command := exec.CommandContext(operation, executable, arguments...)
	for name, value := range environment {
		command.Env = append(command.Env, name+"="+value)
	}
	command.Stdin, command.Stderr = nil, io.Discard
	output := &connectorOutput{limit: int(declaration.MaxOutput)}
	command.Stdout = output
	command.WaitDelay = 2 * time.Second
	cleanup, err := program.StartGroup(command)
	if err != nil {
		return nil, connectorUnavailable()
	}
	defer cleanup()
	if err := command.Wait(); err != nil {
		return nil, connectorUnavailable()
	}
	return slices.Clone(output.Bytes()), nil
}
