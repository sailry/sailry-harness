package main

import "os/exec"

func configureAcceptanceProcess(command *exec.Cmd) {
	command.Cancel = func() error { return command.Process.Kill() }
}

func killAcceptanceProcess(command *exec.Cmd) {
	if command.Process != nil {
		_ = command.Process.Kill()
	}
}
