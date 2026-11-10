package assets

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// open confines file access to one model directory. The directory itself must
// be reached without symbolic links, apart from the Apple system aliases.
func open(path string, create bool) (*os.Root, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return nil, storage(err)
	}
	if runtime.GOOS == "darwin" || runtime.GOOS == "ios" {
		// Apple application-support and temporary paths use these system aliases.
		for _, alias := range [][2]string{{"/var", "/private/var"}, {"/tmp", "/private/tmp"}} {
			if absolute == alias[0] || strings.HasPrefix(absolute, alias[0]+"/") {
				if resolved, err := filepath.EvalSymlinks(alias[0]); err == nil && resolved == alias[1] {
					absolute = alias[1] + strings.TrimPrefix(absolute, alias[0])
				}
				break
			}
		}
	}
	if create {
		if err := os.MkdirAll(absolute, 0o700); err != nil {
			return nil, storage(err)
		}
	}
	resolved, err := filepath.EvalSymlinks(absolute)
	if err != nil {
		return nil, storage(err)
	}
	if resolved != absolute {
		return nil, storageMessage("model asset directory path is invalid")
	}
	root, err := os.OpenRoot(absolute)
	if err != nil {
		return nil, storage(err)
	}
	return root, nil
}

// target accepts a missing name or a regular file that is not a link.
func target(root *os.Root, name string) error {
	info, err := root.Lstat(name)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return storage(err)
	}
	if !info.Mode().IsRegular() {
		return storageMessage("model asset destination is not a regular file")
	}
	return nil
}

// replace writes a private file beside its destination and renames it into
// place, so observers see the previous or the complete content.
func replace(root *os.Root, name string, body []byte) error {
	if err := target(root, name); err != nil {
		return err
	}
	var suffix [8]byte
	if _, err := rand.Read(suffix[:]); err != nil {
		return storage(err)
	}
	temporary := ".tmp-" + hex.EncodeToString(suffix[:])
	file, err := root.OpenFile(temporary, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return storage(err)
	}
	_, err = file.Write(body)
	if err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err == nil {
		err = root.Rename(temporary, name)
	}
	if err != nil {
		_ = root.Remove(temporary)
		return storage(err)
	}
	return nil
}

// valid hashes one file and compares its pinned size and SHA-256. A file that
// changes while it is read is not valid.
func valid(ctx context.Context, root *os.Root, specification File) bool {
	if target(root, specification.Name) != nil {
		return false
	}
	file, err := root.Open(specification.Name)
	if err != nil {
		return false
	}
	defer file.Close()
	before, err := file.Stat()
	if err != nil || !before.Mode().IsRegular() || uint64(before.Size()) != specification.Size {
		return false
	}
	buffer := make([]byte, 64*1024)
	hash := sha256.New()
	var received uint64
	for {
		if ctx.Err() != nil {
			return false
		}
		count, err := file.Read(buffer)
		received += uint64(count)
		if received > specification.Size {
			return false
		}
		hash.Write(buffer[:count])
		if err == io.EOF {
			break
		}
		if err != nil {
			return false
		}
	}
	after, err := file.Stat()
	return err == nil && received == specification.Size && before.Size() == after.Size() && before.ModTime().Equal(after.ModTime()) &&
		strings.EqualFold(hex.EncodeToString(hash.Sum(nil)), specification.SHA256)
}
