package hostctl

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const FollowUsage = "Usage: sailry-host follow status|sync [--data-dir <profile>]\n       sailry-host follow source <NodeID|local|none> [--data-dir <profile>]\n       sailry-host follow target <NodeID> [--configuration on|off] [--secrets on|off] [--data-dir <profile>]\n       sailry-host follow mcp-local <plugin> <server> on|off [--data-dir <profile>]\n       sailry-host follow detach [category] [--data-dir <profile>]\nDisable following before detaching copies. Writes JSON; unknown outcomes retain the original request ID."

type FollowOptions struct {
	Profile       string
	Action        string
	Arguments     []string
	Configuration *bool
	Secrets       *bool
}

func ParseFollow(args []string) (FollowOptions, bool, error) {
	options := FollowOptions{Action: "status"}
	positional := []string{}
	seen := map[string]bool{}
	for index := 0; index < len(args); index++ {
		arg := args[index]
		switch arg {
		case "--help", "-h":
			return options, true, nil
		case "--data-dir", "--configuration", "--secrets":
			if seen[arg] || index+1 == len(args) {
				return options, false, errors.New(FollowUsage)
			}
			seen[arg] = true
			index++
			value := args[index]
			if arg == "--data-dir" {
				if !filepath.IsAbs(value) {
					return options, false, errors.New("--data-dir must be absolute")
				}
				options.Profile = value
				continue
			}
			if value != "on" && value != "off" {
				return options, false, errors.New(FollowUsage)
			}
			enabled := value == "on"
			if arg == "--configuration" {
				options.Configuration = &enabled
			} else {
				options.Secrets = &enabled
			}
		default:
			positional = append(positional, arg)
		}
	}
	if len(positional) > 0 {
		options.Action = positional[0]
		options.Arguments = positional[1:]
	}
	count := len(options.Arguments)
	valid := false
	switch options.Action {
	case "status", "sync":
		valid = count == 0
	case "source", "target":
		valid = count == 1
	case "detach":
		valid = count <= 1
	case "mcp-local":
		valid = count == 3 && (options.Arguments[2] == "on" || options.Arguments[2] == "off")
	}
	if !valid || options.Action != "target" && (options.Configuration != nil || options.Secrets != nil) {
		return options, false, errors.New(FollowUsage)
	}
	if options.Profile == "" {
		var err error
		options.Profile, err = core.DefaultDataDir()
		if err != nil {
			return options, false, err
		}
	}
	return options, false, nil
}
func RunFollow(ctx context.Context, args []string, stdout io.Writer) error {
	options, help, err := ParseFollow(args)
	if err != nil {
		return err
	}
	if help {
		_, err := fmt.Fprintln(stdout, FollowUsage)
		return err
	}
	transport, err := Connect(ctx, options.Profile)
	if err != nil {
		return err
	}
	controller := client.New(transport)
	defer controller.Close()
	status, err := controller.ReadConfigurationFollow(ctx)
	if err != nil {
		return err
	}
	if options.Action == "status" {
		return json.NewEncoder(stdout).Encode(status)
	}
	command := protocol.ConfigurationFollowCommand{ExpectedRevision: status.Revision}
	switch options.Action {
	case "source":
		command.Action = "set_source"
		value := options.Arguments[0]
		if value != "none" {
			id := transport.Target()
			if value != "local" {
				id, err = protocol.ParseNodeID(value)
				if err != nil {
					return err
				}
			}
			command.Source = &id
		}
	case "target":
		command.Action = "set_target"
		id, err := protocol.ParseNodeID(options.Arguments[0])
		if err != nil {
			return err
		}
		target := protocol.ConfigurationFollowTarget{Host: id, Configuration: true}
		for _, saved := range status.Targets {
			if saved.Host == id {
				target = saved
				break
			}
		}
		if options.Configuration != nil {
			target.Configuration = *options.Configuration
		}
		if options.Secrets != nil {
			target.Secrets = *options.Secrets
		}
		command.Target = &target
		command.ExpectedRevision = target.Revision
	case "mcp-local":
		command.Action = "mcp_local"
		command.Plugin, command.Server, command.Local = options.Arguments[0], options.Arguments[1], options.Arguments[2] == "on"
	case "detach":
		command.Action = "detach"
		if len(options.Arguments) > 0 {
			command.Category = options.Arguments[0]
		}
	case "sync":
		command.Action = "sync"
	}
	request := controller.Prepare(protocol.Command{Kind: "configuration_follow", Data: &command})
	output, err := controller.Execute(ctx, request)
	if err != nil {
		var reported *protocol.Fault
		if errors.As(err, &reported) && reported.Code == protocol.ErrorCodeOutcomeUnknown {
			_ = json.NewEncoder(stdout).Encode(struct {
				Request protocol.RequestID `json:"request"`
				Fault   *protocol.Fault    `json:"fault"`
			}{request.ID, reported})
		}
		return err
	}
	return json.NewEncoder(stdout).Encode(output.Data)
}
