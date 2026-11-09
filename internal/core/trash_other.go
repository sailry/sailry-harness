//go:build !darwin && !linux && !windows

package core

import "github.com/sailry/sailry-harness/internal/protocol"

func trashNative(string) (string, error) {
	return "", fileFault(protocol.ErrorCodeUnavailable, "native Trash is unavailable on this platform")
}
