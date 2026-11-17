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

const FollowUsage = "Usage: sailry-host follow status|sync [--data-dir <profile>]\nWrites JSON; unknown outcomes retain the original request ID."

type FollowOptions struct {
	Profile string
	Action  string
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
		case "--data-dir":
			if seen[arg] || index+1 == len(args) {
				return options, false, errors.New(FollowUsage)
			}
			seen[arg] = true
			index++
			value := args[index]
			if !filepath.IsAbs(value) {
				return options, false, errors.New("--data-dir must be absolute")
			}
			options.Profile = value
		default:
			positional = append(positional, arg)
		}
	}
	if len(positional) > 0 {
		options.Action = positional[0]
	}
	if len(positional) > 1 || options.Action != "status" && options.Action != "sync" {
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
	command := protocol.ConfigurationFollowCommand{Action: "sync"}
	request := controller.Prepare(protocol.Command{Kind: "configuration_follow", Data: &command})
	output, err := controller.Execute(ctx, request)
	if err != nil {
		// The client reports faults as values and as link errors; asFault
		// reads every form.
		if reported := asFault(err); reported.Code == protocol.ErrorCodeOutcomeUnknown {
			_ = json.NewEncoder(stdout).Encode(struct {
				Request protocol.RequestID `json:"request"`
				Fault   *protocol.Fault    `json:"fault"`
			}{request.ID, reported})
		}
		return err
	}
	return json.NewEncoder(stdout).Encode(output.Data)
}
