package hostctl

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const IdentityUsage = "Usage: sailry-host users list [--data-dir <profile>]\n       sailry-host devices list|revoke <device id> [--confirm-last] [--data-dir <profile>] [--request-id <id>]"

type identityOptions struct {
	profile string
	request *protocol.RequestID
	action  protocol.IdentityCommand
}

func parseIdentity(args []string) (identityOptions, error) {
	var options identityOptions
	positionals := []string{}
	confirmLast := false
	seen := map[string]bool{}
	for index := 0; index < len(args); index++ {
		flag := args[index]
		if flag != "--data-dir" && flag != "--request-id" && flag != "--confirm-last" {
			positionals = append(positionals, flag)
			continue
		}
		if seen[flag] {
			return options, errors.New("repeated identity option")
		}
		seen[flag] = true
		if flag == "--confirm-last" {
			confirmLast = true
			continue
		}
		index++
		if index == len(args) {
			return options, errors.New(flag + " requires a value")
		}
		if flag == "--data-dir" {
			options.profile = args[index]
			if !filepath.IsAbs(options.profile) {
				return options, errors.New("--data-dir must be absolute")
			}
		} else {
			id, err := protocol.ParseRequestID(args[index])
			if err != nil {
				return options, err
			}
			options.request = &id
		}
	}
	if len(positionals) < 2 {
		return options, errors.New(IdentityUsage)
	}
	category, action := positionals[0], positionals[1]
	switch {
	case (category == "users" || category == "devices") && action == "list" && len(positionals) == 2:
		options.action = protocol.IdentityCommand{Kind: "read_identity"}
	case category == "devices" && action == "revoke" && len(positionals) == 3:
		body, err := hex.DecodeString(positionals[2])
		if err != nil || len(body) != len(protocol.NodeID{}) {
			return options, errors.New("invalid device id")
		}
		options.action = protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: protocol.NodeID(body), ConfirmLast: confirmLast}}
	default:
		return options, errors.New(IdentityUsage)
	}
	if confirmLast && options.action.Kind != "revoke_device" {
		return options, errors.New("--confirm-last requires devices revoke")
	}
	if options.request != nil && options.action.ReadOnly() {
		return options, errors.New("--request-id requires an identity mutation")
	}
	return options, nil
}

// RunIdentity uses the owner-only Host control transport and the same durable
// admission as remote controllers. Uncertain results include their original ID.
func RunIdentity(ctx context.Context, args []string, stdout io.Writer) (int, error) {
	options, err := parseIdentity(args)
	if err != nil {
		return 1, err
	}
	if options.profile == "" {
		options.profile, err = core.DefaultDataDir()
		if err != nil {
			return 1, err
		}
	}
	transport, err := Connect(ctx, options.profile)
	if err != nil {
		return 1, err
	}
	controller := client.New(transport)
	defer controller.Close()
	request := controller.Prepare(protocol.Command{Kind: "identity", Data: options.action})
	if options.request != nil {
		request.ID = *options.request
	}
	var output protocol.Output
	if request.Command.Durable() {
		output, err = controller.Settle(ctx, request)
	} else {
		output, err = controller.Execute(ctx, request)
	}
	result := struct {
		Request protocol.RequestID `json:"request"`
		Output  *protocol.Output   `json:"output,omitempty"`
		Error   string             `json:"error,omitempty"`
	}{Request: request.ID}
	code := 0
	if err == nil {
		result.Output = &output
	} else {
		result.Error = err.Error()
		code = 1
		if client.Retained(err) {
			code = 4
		}
	}
	if writeErr := json.NewEncoder(stdout).Encode(result); writeErr != nil {
		return 1, writeErr
	}
	return code, nil
}
