//go:build darwin || linux

package process

import (
	"errors"
	"io"
	"os"
	"os/exec"
	"sync"
	"syscall"

	"github.com/creack/pty/v2"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func launchNative(launch Launch, environment Environment, captureManaged func(string) (string, error)) (_ *nativeProcess, failure error) {
	directory, err := commandDirectory(launch)
	if err != nil {
		return nil, err
	}
	var master, slave *os.File
	var device string
	if launch.Pty {
		master, slave, err = pty.Open()
		if err != nil {
			return nil, err
		}
		device = slave.Name()
		if err := pty.Setsize(master, &pty.Winsize{Rows: ptyRows, Cols: ptyColumns}); err != nil {
			master.Close()
			slave.Close()
			return nil, err
		}
	}
	command, cleanup, err := prepareCommand(launch, environment, directory, device, captureManaged)
	if err != nil {
		if master != nil {
			master.Close()
			slave.Close()
		}
		return nil, err
	}
	var owned []io.Closer
	owned = append(owned, closerFunc(func() error { cleanup(); return nil }))
	if master != nil {
		owned = append(owned, master, slave)
	}
	var once sync.Once
	closeAll := func() {
		once.Do(func() {
			for _, stream := range owned {
				_ = stream.Close()
			}
		})
	}
	defer func() {
		if failure != nil {
			closeAll()
		}
	}()
	var stdout, stderr io.ReadCloser
	var stdin io.WriteCloser
	if launch.Pty {
		command.Stdin, command.Stdout, command.Stderr = slave, slave, slave
		command.SysProcAttr = &syscall.SysProcAttr{Setsid: true, Setctty: true, Ctty: 0}
		stdout, stdin = master, master
	} else {
		command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
		outReader, outWriter, err := os.Pipe()
		if err != nil {
			return nil, err
		}
		owned = append(owned, outReader, outWriter)
		errReader, errWriter, err := os.Pipe()
		if err != nil {
			return nil, err
		}
		owned = append(owned, errReader, errWriter)
		command.Stdout, command.Stderr = outWriter, errWriter
		stdout, stderr = outReader, errReader
		if launch.Background {
			inReader, inWriter, err := os.Pipe()
			if err != nil {
				return nil, err
			}
			owned = append(owned, inReader, inWriter)
			command.Stdin, stdin = inReader, inWriter
		}
	}
	if err := command.Start(); err != nil {
		return nil, err
	}
	if slave != nil {
		_ = slave.Close()
	} else {
		_ = command.Stdout.(*os.File).Close()
		_ = command.Stderr.(*os.File).Close()
		if command.Stdin != nil {
			_ = command.Stdin.(*os.File).Close()
		}
	}
	id := command.Process.Pid
	signal := func(value syscall.Signal) error {
		err := syscall.Kill(-id, value)
		if errors.Is(err, syscall.ESRCH) || errors.Is(err, syscall.EPERM) && nativeGroupSettled(id) {
			return nil
		}
		return err
	}
	var killOnce sync.Once
	var killError error
	return &nativeProcess{stdout: stdout, stderr: stderr, stdin: stdin, close: closeAll, group: id,
		kill:      func() error { killOnce.Do(func() { killError = signal(syscall.SIGKILL) }); return killError },
		terminate: func() error { return signal(syscall.SIGTERM) },
		settled:   func() bool { return errors.Is(syscall.Kill(-id, 0), syscall.ESRCH) || nativeGroupSettled(id) },
		wait: func() (protocol.ProcessOutcome, error) {
			err := command.Wait()
			var exited *exec.ExitError
			if err != nil && !errors.As(err, &exited) {
				return protocol.ProcessOutcome{}, err
			}
			status := command.ProcessState.Sys().(syscall.WaitStatus)
			if status.Signaled() {
				return protocol.ProcessOutcome{Kind: "signal", Data: int32(status.Signal())}, nil
			}
			return protocol.ProcessOutcome{Kind: "exited", Data: int32(status.ExitStatus())}, nil
		}}, nil
}

type closerFunc func() error

func (close closerFunc) Close() error { return close() }

func nativeReadEnd(err error, terminal bool) bool {
	return errors.Is(err, os.ErrClosed) || terminal && errors.Is(err, syscall.EIO)
}
