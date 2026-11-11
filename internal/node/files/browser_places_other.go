//go:build !windows

package files

import (
	"os"
	"runtime"
)

func systemPlaces(home string) []place { return userPlaces(runtime.GOOS, home, os.Getenv) }
