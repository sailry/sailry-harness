//go:build !windows

package node

import (
	"errors"
	"os/exec"
	"syscall"
)

func startConnectorCLI(command *exec.Cmd) (func(), error) {
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
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
		return nil, err
	}
	return func() { _ = kill() }, nil
}
