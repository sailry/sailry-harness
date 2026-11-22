package core

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Default is set to the release profile by packaging through -ldflags -X.
var Default = identity.SourceProfile

// Release reports a build whose profile default packaging replaced. Release
// builds load native code only from their package.
func Release() bool { return Default != identity.SourceProfile }

// DefaultDataDir resolves the profile location without creating or opening it.
func DefaultDataDir() (string, error) {
	return identity.ResolveProfile(Default)
}

// Profile holds exclusive Node ownership until Close. Its lock file stays linked.
type Profile struct {
	Path     string
	lock     *os.File
	close    sync.Once
	closeErr error
}

func AcquireProfile(path string) (*Profile, error) {
	if !filepath.IsAbs(path) {
		return nil, invalidProfile(path, "an absolute data directory is required")
	}
	if err := PrivateDirectory(path); err != nil {
		return nil, err
	}
	canonical, err := filepath.EvalSymlinks(path)
	if err != nil {
		return nil, err
	}
	lockPath := filepath.Join(canonical, "node.lock")
	if info, err := os.Lstat(lockPath); err == nil {
		if !info.Mode().IsRegular() {
			return nil, invalidProfile(lockPath, "lock must be a regular file")
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	lock, err := openProfileLock(canonical)
	if err != nil {
		return nil, err
	}
	profile := &Profile{Path: canonical, lock: lock}
	if _, err := os.Stat(filepath.Join(canonical, "peers.json")); err == nil {
		_ = profile.Close()
		return nil, invalidProfile(canonical, "a controller profile cannot be opened as a Node")
	} else if !errors.Is(err, os.ErrNotExist) {
		_ = profile.Close()
		return nil, err
	}
	for _, name := range []string{"storage", "tmp"} {
		if err := PrivateDirectory(filepath.Join(canonical, name)); err != nil {
			_ = profile.Close()
			return nil, err
		}
	}
	return profile, nil
}

func (profile *Profile) Close() error {
	profile.close.Do(func() {
		profile.closeErr = errors.Join(unlockProfile(profile.lock), profile.lock.Close())
	})
	return profile.closeErr
}

func Temporary(profile string) (string, error) {
	directory := filepath.Join(profile, "tmp")
	if err := PrivateDirectory(directory); err != nil {
		return "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: fmt.Sprintf("temporary storage is unavailable: %v", err)}
	}
	return directory, nil
}

func DatabaseFiles(path string) []string {
	return []string{path, strings.TrimSuffix(path, filepath.Ext(path)) + ".db-log", path + "-wal", path + "-tshm"}
}
