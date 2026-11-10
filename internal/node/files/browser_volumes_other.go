//go:build !darwin && !linux && !windows

package files

import "github.com/sailry/sailry-harness/internal/protocol"

func browserVolumes() ([]protocol.FileBrowserLocation, error) { return nil, nil }
