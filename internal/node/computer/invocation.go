package computer

import (
	"bufio"
	"context"
	"errors"
	"io"
	"os"
	"os/exec"
	"sort"
	"strings"
	"time"
)

const maxOutput = 64 * 1024

var (
	errOutputLimit = errors.New("computer helper output exceeds the limit")
	errSpawn       = errors.New("computer helper could not start")
)

// Invocation is the captured helper launch a diagnostic command reuses: the
// helper, its directory, the declared variables and the Node's environment.
type Invocation struct {
	Command     string
	Directory   string
	Env         map[string]string
	Environment []string
}

// execute runs one helper command without input, under the helper's own
// system identity. It returns the bounded standard output, complete or up to
// its first line, and whether the helper exited successfully. A response
// line is accepted before a descendant closes the inherited output.
func execute(ctx context.Context, invocation Invocation, args []string, line bool) ([]byte, bool, error) {
	program, args := Disclaim(invocation.Command, args)
	command := exec.CommandContext(ctx, program, args...)
	command.Dir = invocation.Directory
	command.Env = invocation.Environment
	if command.Env == nil {
		command.Env = os.Environ()
	}
	names := make([]string, 0, len(invocation.Env))
	for name := range invocation.Env {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		command.Env = append(command.Env, name+"="+invocation.Env[name])
	}
	command.WaitDelay = 2 * time.Second
	hideWindow(command)
	output, err := command.StdoutPipe()
	if err != nil {
		return nil, false, errSpawn
	}
	if err := command.Start(); err != nil {
		return nil, false, errSpawn
	}
	limited := io.LimitReader(output, maxOutput+1)
	var body []byte
	if line {
		body, err = bufio.NewReader(limited).ReadBytes('\n')
	} else {
		body, err = io.ReadAll(limited)
	}
	if errors.Is(err, io.EOF) {
		err = nil
	}
	if err == nil && len(body) > maxOutput {
		err = errOutputLimit
	}
	if err != nil {
		_ = command.Process.Kill()
		_ = command.Wait()
		if ctx.Err() != nil {
			return nil, false, ctx.Err()
		}
		return nil, false, err
	}
	waited := command.Wait()
	if ctx.Err() != nil {
		return nil, false, ctx.Err()
	}
	return body, waited == nil, nil
}

// without drops one variable from both the declared and inherited values.
func (invocation Invocation) without(name string) Invocation {
	copy := invocation
	copy.Env = make(map[string]string, len(invocation.Env))
	for key, value := range invocation.Env {
		if key != name {
			copy.Env[key] = value
		}
	}
	environment := invocation.Environment
	if environment == nil {
		environment = os.Environ()
	}
	copy.Environment = make([]string, 0, len(environment))
	for _, assignment := range environment {
		if key, _, _ := strings.Cut(assignment, "="); key != name {
			copy.Environment = append(copy.Environment, assignment)
		}
	}
	return copy
}
