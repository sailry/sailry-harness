package python

import (
	"context"
	"errors"
	"fmt"
	"maps"
	"os"
	"os/exec"
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Command struct {
	Executable  string
	Arguments   []string
	Directory   string
	Environment map[string]string
}

type stderrTail struct {
	mu    sync.Mutex
	value []byte
}

func (tail *stderrTail) append(value []byte) {
	tail.mu.Lock()
	defer tail.mu.Unlock()
	tail.value = append(tail.value, value...)
	if len(tail.value) > 16*1024 {
		tail.value = slices.Clone(tail.value[len(tail.value)-16*1024:])
	}
}

func (tail *stderrTail) text() string {
	tail.mu.Lock()
	defer tail.mu.Unlock()
	return string(tail.value)
}

func Run(ctx context.Context, input Command) error {
	if err := check(ctx); err != nil {
		return err
	}
	command := exec.Command(input.Executable, input.Arguments...)
	command.Dir = input.Directory
	command.Env = make([]string, 0, len(input.Environment))
	for _, name := range slices.Sorted(maps.Keys(input.Environment)) {
		command.Env = append(command.Env, name+"="+input.Environment[name])
	}
	null, err := os.OpenFile(os.DevNull, os.O_RDWR, 0)
	if err != nil {
		return storage(err)
	}
	defer null.Close()
	reader, writer, err := os.Pipe()
	if err != nil {
		return storage(err)
	}
	defer reader.Close()
	defer writer.Close()
	command.Stdin, command.Stdout, command.Stderr = null, null, writer
	group, err := startProcess(command)
	if err != nil {
		return unavailable(fmt.Sprintf("Python preparation could not start: %v", err))
	}
	defer group.close()
	_ = writer.Close()
	var tail stderrTail
	drained := make(chan struct{})
	go func() {
		defer close(drained)
		buffer := make([]byte, 4096)
		for {
			count, err := reader.Read(buffer)
			if count > 0 {
				tail.append(buffer[:count])
			}
			if err != nil {
				return
			}
		}
	}()
	waited := make(chan error, 1)
	go func() { waited <- command.Wait() }()
	timeout := time.NewTimer(20 * time.Minute)
	defer timeout.Stop()
	var result error
	rootDone := false
	select {
	case <-ctx.Done():
		result = cancelled()
	case <-timeout.C:
		result = unavailable("Python preparation timed out")
	case result = <-waited:
		rootDone = true
	}
	killErr := group.kill()
	if !rootDone {
		reap := time.NewTimer(2 * time.Second)
		select {
		case <-waited:
			rootDone = true
		case <-reap.C:
		}
		reap.Stop()
	}
	drain := time.NewTimer(time.Second)
	select {
	case <-drained:
	case <-drain.C:
		_ = reader.Close()
		<-drained
	}
	drain.Stop()
	if !rootDone || killErr != nil {
		return unavailable("Python preparation cleanup could not be confirmed")
	}
	if result != nil {
		var exited *exec.ExitError
		if errors.As(result, &exited) {
			return unavailable(fmt.Sprintf("Python preparation exited with %v: %s", exited, tail.text()))
		}
		var fault *protocol.Fault
		if errors.As(result, &fault) {
			return result
		}
		return unavailable(fmt.Sprintf("Python preparation wait failed: %v", result))
	}
	return check(ctx)
}
