package mcp

import (
	"context"
	"errors"
	"io"
	"os/exec"
	"sync"
	"time"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
)

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
	configureGroup(transport.command)
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
		owner.waitErr = owner.command.Wait()
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
	waitErr error
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
		owner.err = errors.Join(inputErr, owner.waitErr, killErr, groupErr)
	})
	return owner.err
}
