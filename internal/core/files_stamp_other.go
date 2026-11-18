//go:build !darwin

package core

import (
	"io/fs"
	"os"
)

// Identity, size and timestamps alone cannot rule out an equal-length rewrite.
func fileStamp(*os.File, fs.FileInfo) FileStamp { return FileStamp{} }
