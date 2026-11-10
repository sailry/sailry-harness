//go:build !darwin && !linux && !windows

package assets

import "io/fs"

func nativeOf(fs.FileInfo) native { return native{} }
