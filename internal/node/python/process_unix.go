//go:build !windows

package python

import (
	"errors"
	"os/exec"
	"syscall"
)

type processGroup struct{ id int }

func startProcess(command *exec.Cmd) (*processGroup, error) {
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	if err := command.Start(); err != nil {
		return nil, err
	}
	return &processGroup{id: command.Process.Pid}, nil
}

func (group *processGroup) kill() error {
	err := syscall.Kill(-group.id, syscall.SIGKILL)
	if errors.Is(err, syscall.ESRCH) {
		return nil
	}
	return err
}

func (group *processGroup) close() error { return nil }
