//go:build !unix && !windows

package link

import (
	"errors"
	"os"
)

func prepareControllerFiles(string) (string, *os.File, error) {
	return "", nil, errors.New("controller profile protection is unavailable on this platform")
}

func readControllerFile(string) (*os.File, error) {
	return nil, errors.New("controller profile protection is unavailable on this platform")
}

func validateControllerIdentity(string) error { return nil }

func replaceControllerFile(source, target string) error { return os.Rename(source, target) }

func syncControllerDirectory(string) error { return nil }
