//go:build !linux || android

package main

import (
	"context"
	"errors"
	"io"
)

func runPlatformDaemon(context.Context, string, []string, io.Writer) error {
	return errors.New("daemon requires Linux with systemd; use start to run the relay in the foreground")
}
