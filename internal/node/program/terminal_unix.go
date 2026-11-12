//go:build darwin || linux

package program

import (
	"errors"
	"os"
	"sync"
	"syscall"

	"github.com/creack/pty/v2"
)

// Terminal reports whether StartTerminal works on this system.
const Terminal = true

// terminal is the Node's end of a program's terminal. It closes once: when
// the program is killed, or when a read finds the program gone.
type terminal struct {
	file *os.File
	once sync.Once
}

func (terminal *terminal) Write(data []byte) (int, error) { return terminal.file.Write(data) }

func (terminal *terminal) Read(data []byte) (int, error) {
	count, err := terminal.file.Read(data)
	if err != nil {
		_ = terminal.Close()
	}
	return count, err
}

func (terminal *terminal) Close() error {
	var err error
	terminal.once.Do(func() { err = terminal.file.Close() })
	return err
}

// StartTerminal is Start for a program that needs a terminal instead of
// pipes: it runs as the leader of its own session with a pseudo-terminal of
// this size as its controlling terminal. Stdin and Stdout are the Node's end
// of that terminal, and the program's error output is part of what it draws.
func StartTerminal(spec Spec, columns, rows uint16) (*Process, error) {
	command, err := command(spec)
	if err != nil {
		return nil, err
	}
	master, slave, err := pty.Open()
	if err != nil {
		return nil, err
	}
	defer slave.Close()
	if err := pty.Setsize(master, &pty.Winsize{Rows: rows, Cols: columns}); err != nil {
		_ = master.Close()
		return nil, err
	}
	command.Stdin, command.Stdout, command.Stderr = slave, slave, slave
	// A session leader leads its own process group too, so one signal to the
	// group ends the program and its children.
	command.SysProcAttr = &syscall.SysProcAttr{Setsid: true, Setctty: true, Ctty: 0}
	kill := func() error {
		if command.Process == nil {
			return nil
		}
		err := syscall.Kill(-command.Process.Pid, syscall.SIGKILL)
		if errors.Is(err, syscall.ESRCH) {
			return nil
		}
		return err
	}
	command.Cancel = kill
	if err := command.Start(); err != nil {
		_ = master.Close()
		return nil, err
	}
	stream := &terminal{file: master}
	process := &Process{Stdin: stream, Stdout: stream, kill: func() { _ = kill() }, done: make(chan struct{})}
	go func() {
		_ = command.Wait()
		_ = kill()
		close(process.done)
	}()
	return process, nil
}
