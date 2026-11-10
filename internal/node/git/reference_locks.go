package git

import (
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// go-git serializes its own writers with descriptor locks. Native Git uses
// sibling .lock files instead, so retain those source locks while using go-git
// reference/config plumbing; this prevents native writers bypassing revisions.
func lockReferences(r *gogit.Repository, names []string) (func(), error) {
	private, err := gitDir("", r)
	if err != nil {
		return nil, err
	}
	common, err := commonDir(r)
	if err != nil {
		return nil, err
	}
	paths := map[string]bool{}
	for _, name := range names {
		base := common
		if name == "HEAD" {
			base = private
		}
		paths[filepath.Join(base, filepath.FromSlash(name)+".lock")] = true
	}
	ordered := []string{}
	for p := range paths {
		ordered = append(ordered, p)
	}
	sort.Strings(ordered)
	type retained struct {
		root *os.Root
		file *os.File
		name string
		info os.FileInfo
	}
	owned := []retained{}
	release := func() {
		for i := len(owned) - 1; i >= 0; i-- {
			value := owned[i]
			current, err := value.root.Lstat(value.name)
			if err == nil && os.SameFile(current, value.info) {
				_ = value.root.Remove(value.name)
			}
			value.file.Close()
			value.root.Close()
		}
	}
	for _, path := range ordered {
		base := common
		if pathWithin(path, private) && !pathWithin(path, common) {
			base = private
		}
		relative, err := filepath.Rel(base, path)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			release()
			return nil, invalid("Git reference lock is outside metadata")
		}
		anchor, err := os.OpenRoot(base)
		if err != nil {
			release()
			return nil, mapError(err)
		}
		if parent := filepath.Dir(relative); parent != "." {
			if err := anchor.MkdirAll(parent, 0755); err != nil {
				anchor.Close()
				release()
				return nil, mapError(err)
			}
		}
		file, err := anchor.OpenFile(relative, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			anchor.Close()
			release()
			if errors.Is(err, os.ErrExist) {
				return nil, fault(protocol.ErrorCodeBusy, "Git reference is locked")
			}
			return nil, mapError(err)
		}
		info, err := file.Stat()
		if err != nil {
			file.Close()
			anchor.Remove(relative)
			anchor.Close()
			release()
			return nil, mapError(err)
		}
		owned = append(owned, retained{anchor, file, relative, info})
	}
	return release, nil
}
