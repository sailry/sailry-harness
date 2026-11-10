package process

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const cleanupTimeout = 2 * time.Second

type nativeProcess struct {
	stdout       io.ReadCloser
	stderr       io.ReadCloser
	stdin        io.WriteCloser
	wait         func() (protocol.ProcessOutcome, error)
	kill         func() error
	terminate    func() error
	settled      func() bool
	close        func()
	finishOutput func()
}

func commandDirectory(launch Launch) (string, error) {
	root, err := filepath.EvalSymlinks(launch.Root)
	if err != nil {
		return "", processFailure(err)
	}
	directory := launch.Cwd
	if !filepath.IsAbs(directory) {
		directory = filepath.Join(root, directory)
	}
	directory, err = filepath.EvalSymlinks(directory)
	if err != nil {
		return "", invalid(fmt.Sprintf("command working directory is unavailable: %v", err))
	}
	info, err := os.Stat(directory)
	if err != nil || !info.IsDir() {
		return "", invalid("command working directory is not a directory")
	}
	if launch.Access == protocol.ProcessAccessWorkspace && (runtime.GOOS == "darwin" || runtime.GOOS == "linux") && !within(root, directory) {
		return "", &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "command working directory is outside the workspace"}
	}
	return directory, nil
}

func within(parent, path string) bool {
	relative, err := filepath.Rel(parent, path)
	return err == nil && relative != ".." && !filepath.IsAbs(relative) && (len(relative) < 3 || relative[:3] != ".."+string(filepath.Separator))
}

func (s *Service) executeNative(ctx context.Context, launch Launch, emit func(protocol.ProcessOutputOutputStream, []byte), input <-chan []byte) (protocol.ProcessCompletion, error) {
	started := time.Now()
	completion := protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}
	if ctx.Err() != nil || s.ctx.Err() != nil {
		return completion, nil
	}
	child, err := launchNative(launch, s.options.Environment, s.options.CaptureManaged)
	if err != nil {
		return completion, processFailure(err)
	}
	defer child.close()
	defer child.kill()
	type result struct {
		outcome protocol.ProcessOutcome
		err     error
	}
	waited := make(chan result, 1)
	go func() { outcome, err := child.wait(); waited <- result{outcome, err} }()
	var drains sync.WaitGroup
	drainErrors := make(chan error, 2)
	for _, stream := range []struct {
		reader io.ReadCloser
		kind   protocol.ProcessOutputOutputStream
	}{{child.stdout, protocol.ProcessOutputOutputStreamStdout}, {child.stderr, protocol.ProcessOutputOutputStreamStderr}} {
		if stream.reader == nil {
			continue
		}
		drains.Add(1)
		go func() {
			defer drains.Done()
			buffer := make([]byte, 32*1024)
			for {
				count, err := stream.reader.Read(buffer)
				if count > 0 {
					emit(stream.kind, buffer[:count])
				}
				if err != nil {
					if !errors.Is(err, io.EOF) && !nativeReadEnd(err, launch.Pty) {
						drainErrors <- err
					}
					return
				}
			}
		}()
	}
	inputContext, stopInput := context.WithCancel(ctx)
	inputErrors := make(chan error, 1)
	inputDone := make(chan struct{})
	go func() {
		defer close(inputDone)
		for input != nil && child.stdin != nil {
			select {
			case <-inputContext.Done():
				return
			case value, ok := <-input:
				if !ok {
					return
				}
				if _, err := child.stdin.Write(value); err != nil {
					inputErrors <- err
					return
				}
			}
		}
	}()
	var timeout <-chan time.Time
	var timer *time.Timer
	if !launch.Background {
		timer = time.NewTimer(time.Duration(launch.TimeoutMs) * time.Millisecond)
		defer timer.Stop()
		timeout = timer.C
	}
	rootDone := false
	select {
	case <-s.ctx.Done():
	case <-ctx.Done():
	case <-timeout:
		completion.Outcome.Kind = "timed_out"
	case failure := <-inputErrors:
		completion.Outcome = unknownOutcome("command input failed", failure)
	case result := <-waited:
		rootDone = true
		completion.Outcome = result.outcome
		if result.err != nil {
			completion.Outcome = unknownOutcome("command wait failed", result.err)
		}
	}
	stopInput()
	// Stop permits the source grace period; Node closure immediately kills the group.
	if ctx.Err() != nil && s.ctx.Err() == nil && child.terminate != nil && child.terminate() == nil {
		deadline := time.NewTimer(cleanupTimeout)
		tick := time.NewTicker(20 * time.Millisecond)
	grace:
		for {
			if rootDone && child.settled() {
				break
			}
			select {
			case <-s.ctx.Done():
				break grace
			case <-deadline.C:
				break grace
			case <-tick.C:
			case <-waited:
				rootDone = true
			}
		}
		deadline.Stop()
		tick.Stop()
	}
	// Descendants are terminated even when the root exited successfully.
	if err := child.kill(); err != nil {
		completion.Outcome = unknownOutcome("command cleanup failed", err)
	}
	if child.stdin != nil && !launch.Pty {
		_ = child.stdin.Close()
	}
	if !rootDone {
		deadline := time.NewTimer(cleanupTimeout)
		select {
		case result := <-waited:
			if result.err != nil {
				completion.Outcome = unknownOutcome("command reap failed", result.err)
			}
		case <-deadline.C:
			completion.Outcome = protocol.ProcessOutcome{Kind: "unknown", Data: "command cleanup timed out"}
		}
		deadline.Stop()
	}
	if child.finishOutput != nil {
		child.finishOutput()
	}
	drained := make(chan struct{})
	go func() { drains.Wait(); close(drained) }()
	deadline := time.NewTimer(cleanupTimeout)
	select {
	case <-drained:
	case <-deadline.C:
		child.close()
		<-drained
		completion.Outcome = protocol.ProcessOutcome{Kind: "unknown", Data: "command output cleanup timed out"}
	}
	deadline.Stop()
	child.close()
	<-inputDone
	select {
	case failure := <-drainErrors:
		completion.Outcome = unknownOutcome("command output failed", failure)
	default:
	}
	completion.ElapsedMs = uint64(time.Since(started).Milliseconds())
	return completion, nil
}

func unknownOutcome(message string, failure error) protocol.ProcessOutcome {
	return protocol.ProcessOutcome{Kind: "unknown", Data: message + ": " + failure.Error()}
}
