//go:build unix

package git

import "os"

func syncDirectory(path string) error {
	dir, err := os.Open(path)
	if err != nil {
		return mapError(err)
	}
	defer dir.Close()
	return mapError(dir.Sync())
}
