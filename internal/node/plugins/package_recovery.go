package plugins

import (
	"context"
	"crypto/rand"
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var (
	errPackageUnavailable = packageFault("immutable plugin package is unavailable")
	errPackageChanged     = packageFault("immutable plugin package content changed")
)

// RestoreBundled repairs only a digest that this build can reproduce from its
// embedded package. A damaged published entry is retained outside the digest namespace.
func (manager *Manager) RestoreBundled(ctx context.Context, reference protocol.PluginReference) (protocol.PluginInfo, error) {
	entries, issues, err := captureBundled(ctx, manager.bundledFiles(), reference.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if len(issues) != 0 || digestPackage(entries) != reference.Digest {
		return protocol.PluginInfo{}, packageFault("matching bundled plugin package is unavailable")
	}
	return manager.publishPackage(ctx, entries, issues, reference.Name, protocol.PluginOrigin{Kind: "bundled"}, true)
}

func (manager *Manager) publishEntries(ctx context.Context, entries []packageEntry, digest string, repair bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := os.MkdirAll(manager.profile, 0o700); err != nil {
		return err
	}
	profile, err := os.OpenRoot(manager.profile)
	if err != nil {
		return err
	}
	defer profile.Close()
	plugins, err := createPackageDirectory(profile, "plugins")
	if err != nil {
		return err
	}
	defer plugins.Close()
	packages, err := createPackageDirectory(plugins, "packages")
	if err != nil {
		return err
	}
	defer packages.Close()

	old, err := packages.Lstat(digest)
	if err == nil {
		err = verifyPublished(ctx, packages, digest, digest)
		if err == nil {
			return nil
		}
		if !repair || !errors.Is(err, errPackageUnavailable) && !errors.Is(err, errPackageChanged) {
			return err
		}
	} else if !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	id := rand.Text()
	staging := ".staging-" + id
	if err := packages.Mkdir(staging, 0o700); err != nil {
		return err
	}
	defer packages.RemoveAll(staging)
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			return err
		}
		filename := filepath.Join(staging, filepath.FromSlash(entry.name))
		if entry.directory {
			err = packages.MkdirAll(filename, 0o700)
		} else {
			mode := fs.FileMode(0o600)
			if entry.executable {
				mode = 0o700
			}
			err = packages.WriteFile(filename, entry.data, mode)
		}
		if err != nil {
			return err
		}
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if repair {
		if err := verifyPublished(ctx, packages, staging, digest); err != nil {
			return err
		}
	}
	if old != nil {
		current, err := packages.Lstat(digest)
		if err != nil {
			return err
		}
		if !os.SameFile(old, current) {
			return verifyPublished(ctx, packages, digest, digest)
		}
		if err := packages.Rename(digest, ".quarantine-"+digest+"-"+id); err != nil {
			return err
		}
	}
	if err := packages.Rename(staging, digest); err != nil {
		if verifyErr := verifyPublished(ctx, packages, digest, digest); verifyErr != nil {
			return err
		}
	}
	return nil
}

func createPackageDirectory(parent *os.Root, name string) (*os.Root, error) {
	if err := parent.Mkdir(name, 0o700); err != nil && !errors.Is(err, fs.ErrExist) {
		return nil, err
	}
	info, err := parent.Lstat(name)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, packageFault("plugin package store is unavailable")
	}
	directory, err := parent.OpenRoot(name)
	if err != nil {
		return nil, err
	}
	opened, err := directory.Stat(".")
	if err == nil && !os.SameFile(info, opened) {
		err = packageFault("plugin package store moved while being opened")
	}
	if err != nil {
		directory.Close()
		return nil, err
	}
	return directory, nil
}
