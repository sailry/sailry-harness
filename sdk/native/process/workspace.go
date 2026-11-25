package process

import "github.com/sailry/sailry-harness/internal/node/process"

func CaptureWorkspaceBoundary(root string) (string, error) {
	return process.CaptureWorkspaceBoundary(root)
}
