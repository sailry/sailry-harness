package mcp

import (
	"context"
	"errors"
	"io"
	"maps"
	"os"
	"os/exec"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func stdioCommand(launch Launch) (*exec.Cmd, error) {
	environment := slices.Clone(launch.Environment)
	if environment == nil {
		environment = os.Environ()
	}
	executable := launch.Command
	if !strings.ContainsAny(executable, `/\`) {
		// The Node's captured shell environment owns executable lookup.
		// A server's environment overrides are passed to the child only.
		search := map[string]string{}
		for _, entry := range environment {
			name, value, ok := strings.Cut(entry, "=")
			if runtime.GOOS == "windows" {
				name = strings.ToUpper(name)
			}
			if ok && (name == "PATH" || name == "PATHEXT") {
				search[name] = value
			}
		}
		path, err := program.Resolve(executable, search)
		if err != nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP executable was not found in the Node's PATH"}
		}
		executable = path
	}
	command := exec.Command(executable, launch.Args...)
	command.Args[0], command.Dir, command.Stderr = launch.Command, launch.Directory, io.Discard
	for _, name := range slices.Sorted(maps.Keys(launch.Env)) {
		environment = append(environment, name+"="+launch.Env[name])
	}
	command.Env = environment
	if err := configureCommand(command); err != nil {
		return nil, err
	}
	return command, nil
}

type commandTransport struct{ command *exec.Cmd }

func (transport commandTransport) Connect(ctx context.Context) (sdk.Connection, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	output, err := transport.command.StdoutPipe()
	if err != nil {
		return nil, err
	}
	input, err := transport.command.StdinPipe()
	if err != nil {
		_ = output.Close()
		return nil, err
	}
	if err := transport.command.Start(); err != nil {
		_ = output.Close()
		_ = input.Close()
		return nil, err
	}
	group, err := retainGroup(transport.command)
	if err != nil {
		_ = transport.command.Process.Kill()
		_ = transport.command.Wait()
		_ = output.Close()
		_ = input.Close()
		return nil, err
	}
	owner := &commandOwner{command: transport.command, input: input, output: output, group: group, waited: make(chan struct{})}
	go func() {
		_ = owner.command.Wait()
		close(owner.waited)
	}()
	wire, err := (&sdk.IOTransport{Reader: io.NopCloser(output), Writer: owner, MaxLineLength: maxMessage}).Connect(ctx)
	if err != nil {
		_ = owner.Close()
		return nil, err
	}
	return wire, nil
}

type commandOwner struct {
	command *exec.Cmd
	input   io.WriteCloser
	output  io.ReadCloser
	group   *commandGroup
	waited  chan struct{}
	once    sync.Once
	err     error
}

func (owner *commandOwner) Write(body []byte) (int, error) { return owner.input.Write(body) }

func (owner *commandOwner) Close() error {
	owner.once.Do(func() {
		inputErr := owner.input.Close()
		wait := func(duration time.Duration) bool {
			timer := time.NewTimer(duration)
			defer timer.Stop()
			select {
			case <-owner.waited:
				return true
			case <-timer.C:
				return false
			}
		}
		settled := wait(5 * time.Second)
		if !settled {
			_ = owner.group.terminate()
			settled = wait(3 * time.Second)
		}
		killErr := owner.group.kill()
		if !settled {
			settled = wait(2 * time.Second)
		}
		_ = owner.output.Close()
		groupSettled := owner.group.settled()
		groupErr := owner.group.close()
		if !settled || !groupSettled {
			owner.err = errors.New("MCP subprocess cleanup could not be confirmed")
			return
		}
		// How the child exited is not a cleanup failure: a server ended by
		// the signal above, or one that exits non-zero when its input
		// closes, is gone all the same.
		owner.err = errors.Join(inputErr, killErr, groupErr)
	})
	return owner.err
}
