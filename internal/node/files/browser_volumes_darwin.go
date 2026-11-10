package files

import (
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/unix"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

func browserVolumes() ([]protocol.FileBrowserLocation, error) {
	count, err := unix.Getfsstat(nil, unix.MNT_NOWAIT)
	if err != nil {
		return nil, err
	}
	mounts := make([]unix.Statfs_t, count)
	count, err = unix.Getfsstat(mounts, unix.MNT_NOWAIT)
	if err != nil {
		return nil, err
	}
	var out []protocol.FileBrowserLocation
	for _, mount := range mounts[:min(count, len(mounts))] {
		if mount.Flags&unix.MNT_LOCAL == 0 || mount.Flags&unix.MNT_DONTBROWSE != 0 {
			continue
		}
		data := make([]byte, 0, len(mount.Mntonname))
		for _, value := range mount.Mntonname {
			if value == 0 {
				break
			}
			data = append(data, byte(value))
		}
		path := string(data)
		if !utf8.ValidString(path) || !filepath.IsAbs(path) || strings.HasPrefix(path, "/System/Volumes") || strings.HasPrefix(path, "/private/var/vm") {
			continue
		}
		out = append(out, protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: filepath.Base(path), Path: path})
	}
	return out, nil
}
