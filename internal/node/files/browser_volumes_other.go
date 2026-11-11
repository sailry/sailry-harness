//go:build !windows

package files

import "github.com/sailry/sailry-harness/internal/protocol"

func driveDetail(string) (string, *protocol.FileBrowserMedium) { return "", nil }
