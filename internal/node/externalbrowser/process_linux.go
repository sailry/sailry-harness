package externalbrowser

import (
	"os/exec"
	"syscall"
)

// The kernel kills the browser when the Node dies without closing it.
func configurePlatform(command *exec.Cmd) { command.SysProcAttr.Pdeathsig = syscall.SIGKILL }
