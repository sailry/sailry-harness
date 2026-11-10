//go:build !windows

package mcp

import (
	"errors"
	"os/exec"
	"syscall"
)

type commandGroup struct{ id int }

func configureGroup(command *exec.Cmd) { command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true} }

func retainGroup(command *exec.Cmd) (*commandGroup, error) {
	return &commandGroup{id: command.Process.Pid}, nil
}

func (group *commandGroup) signal(signal syscall.Signal) error {
	err := syscall.Kill(-group.id, signal)
	if errors.Is(err, syscall.ESRCH) {
		return nil
	}
	return err
}

func (group *commandGroup) terminate() error { return group.signal(syscall.SIGTERM) }
func (group *commandGroup) kill() error      { return group.signal(syscall.SIGKILL) }
func (group *commandGroup) settled() bool {
	return errors.Is(syscall.Kill(-group.id, 0), syscall.ESRCH)
}
func (group *commandGroup) close() error { return nil }
