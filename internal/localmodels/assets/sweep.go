package assets

import (
	"errors"
	"io/fs"
	"os"
	"strings"
	"time"
)

// PartRetention is how long an interrupted download stays resumable.
const PartRetention = 7 * 24 * time.Hour

// Sweep removes what interrupted transfers left in one model directory and
// can no longer be used: temporary files, partial downloads of files the
// model does not have, partial downloads without a checkpoint that could
// continue them, and partial downloads last written before stale. A recent
// partial download with its checkpoint stays for the resume. Nothing may be
// transferring into the directory. What cannot be removed is left for the
// next sweep.
func Sweep(directory string, files []File, stale time.Time) {
	if _, err := os.Lstat(directory); err != nil {
		return
	}
	root, err := open(directory, false)
	if err != nil {
		return
	}
	defer root.Close()
	entries, err := fs.ReadDir(root.FS(), ".")
	if err != nil {
		return
	}
	parts := map[string]File{}
	for _, file := range files {
		parts[partName(file)] = file
	}
	for _, entry := range entries {
		// Pinned file names never start with a dot.
		name := entry.Name()
		switch {
		case strings.HasPrefix(name, ".tmp-"):
			_ = root.Remove(name)
		case strings.HasPrefix(name, ".") && strings.HasSuffix(name, ".part"):
			file, expected := parts[name]
			info, err := entry.Info()
			if expected && err == nil && info.Mode().IsRegular() && !info.ModTime().Before(stale) {
				if saved := readCheckpoint(root, name+".json"); saved.usable(file.Size, uint64(info.Size())) && saved.Received > 0 {
					continue
				}
			}
			_ = root.Remove(name)
			_ = root.Remove(name + ".json")
		case strings.HasPrefix(name, ".") && strings.HasSuffix(name, ".part.json"):
			if _, err := root.Lstat(strings.TrimSuffix(name, ".json")); errors.Is(err, fs.ErrNotExist) {
				_ = root.Remove(name)
			}
		}
	}
}
