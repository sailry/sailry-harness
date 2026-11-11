//go:build !windows

package externalbrowser

import (
	"os"
	"os/exec"
	"syscall"
)

// processGroup is the process group the browser leads.
type processGroup struct{ id int }

// configureProcess starts the browser as the leader of its own group.
func configureProcess(command *exec.Cmd) {
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	configurePlatform(command)
}

func retainProcess(process *os.Process) (*processGroup, error) {
	return &processGroup{id: process.Pid}, nil
}

func (group *processGroup) kill() { _ = syscall.Kill(-group.id, syscall.SIGKILL) }
