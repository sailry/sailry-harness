//go:build !windows

package main

import (
	"os/exec"
	"syscall"
)

func configureAcceptanceProcess(command *exec.Cmd) {
	// The isolated acceptance child owns any processes it starts. Interrupt
	// that whole group so an inherited stdout pipe cannot outlive cancellation.
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	command.Cancel = func() error { return syscall.Kill(-command.Process.Pid, syscall.SIGINT) }
}

func killAcceptanceProcess(command *exec.Cmd) {
	if command.Process != nil {
		_ = syscall.Kill(-command.Process.Pid, syscall.SIGKILL)
	}
}
