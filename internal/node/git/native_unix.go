//go:build unix

package git

import (
	"os/exec"
	"syscall"
)

func prepareNative(cmd *exec.Cmd) (func(), error) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	return func() {}, nil
}
func ownNative(*exec.Cmd) error { return nil }
func killNative(cmd *exec.Cmd) {
	if cmd.Process != nil && cmd.Process.Pid > 1 {
		_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	}
}
