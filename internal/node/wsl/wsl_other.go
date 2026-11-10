//go:build !windows

package wsl

import (
	"context"
	"os/exec"
)

func discover(context.Context, []string) (source, error) {
	return source{}, unavailable("WSL Host installation requires Windows")
}

func start(command *exec.Cmd) (func(), error) {
	return func() {}, command.Start()
}
