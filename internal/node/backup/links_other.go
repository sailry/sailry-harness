//go:build !unix

package backup

import "io/fs"

func regularDatabase(info fs.FileInfo) error {
	if !info.Mode().IsRegular() {
		return invalid("backup database must be a regular file")
	}
	return nil
}
