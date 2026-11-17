package store

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/core"
)

type schemaPreview struct {
	path      string
	directory string
}

func (preview *schemaPreview) close() error { return os.RemoveAll(preview.directory) }

func previewDatabase(path string) (*schemaPreview, error) {
	return snapshotDatabase(path, filepath.Dir(path))
}

func snapshotDatabase(path, parent string) (_ *schemaPreview, failure error) {
	directory, err := os.MkdirTemp(parent, ".upgrade-")
	if err != nil {
		return nil, err
	}
	defer func() {
		if failure != nil {
			_ = os.RemoveAll(directory)
		}
	}()
	// MkdirTemp reserves only the cleanup container. Database files live in the
	// child with explicit private permissions, including a protected Windows
	// DACL; the container may keep the parent's inherited permissions.
	storage := filepath.Join(directory, "storage")
	if err := core.PrivateDirectory(storage); err != nil {
		return nil, err
	}
	target := filepath.Join(storage, filepath.Base(path))
	if err := copyDatabase(context.Background(), path, target); err != nil {
		return nil, err
	}
	return &schemaPreview{path: target, directory: directory}, nil
}

// Copy keeps every source connection closed while copying the native family.
// Reopen and Close wait for the copy to finish or return its cancellation.
func (closed *Closed) Copy(ctx context.Context, target string) error {
	closed.mu.Lock()
	defer closed.mu.Unlock()
	if closed.finished {
		return core.ErrStopped
	}
	return copyDatabase(ctx, closed.Path(), target)
}

// copyDatabase requires the lifecycle owner to close every handle first.
func copyDatabase(ctx context.Context, source, target string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := checkDatabaseFiles(source); err != nil {
		return err
	}
	sourceRoot, err := os.OpenRoot(filepath.Dir(source))
	if err != nil {
		return err
	}
	defer sourceRoot.Close()
	targetRoot, err := os.OpenRoot(filepath.Dir(target))
	if err != nil {
		return err
	}
	defer targetRoot.Close()
	sources, targets := core.DatabaseFiles(source), core.DatabaseFiles(target)
	for index, sourcePath := range sources {
		if err := ctx.Err(); err != nil {
			return err
		}
		name := filepath.Base(sourcePath)
		before, err := sourceRoot.Lstat(name)
		if errors.Is(err, os.ErrNotExist) && index > 0 {
			continue
		}
		if err != nil {
			return err
		}
		if !before.Mode().IsRegular() || before.Mode()&os.ModeSymlink != 0 {
			return snapshotUnavailable()
		}
		input, err := sourceRoot.Open(name)
		if err != nil {
			return err
		}
		opened, err := input.Stat()
		if err != nil || !os.SameFile(before, opened) {
			_ = input.Close()
			return snapshotUnavailable()
		}
		output, err := targetRoot.OpenFile(filepath.Base(targets[index]), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
		if err != nil {
			_ = input.Close()
			return err
		}
		count, copyErr := io.Copy(output, &databaseCopyReader{ctx: ctx, input: input})
		after, statErr := input.Stat()
		inputErr := input.Close()
		if copyErr == nil && statErr == nil && (count != before.Size() || after.Size() != before.Size() || !after.ModTime().Equal(before.ModTime())) {
			copyErr = errors.New("database changed during snapshot; existing data was not changed")
		}
		var syncErr error
		if copyErr == nil && statErr == nil {
			syncErr = output.Sync()
		}
		if err := errors.Join(copyErr, statErr, inputErr, syncErr, output.Close()); err != nil {
			return err
		}
	}
	return nil
}

type databaseCopyReader struct {
	ctx   context.Context
	input io.Reader
}

func (reader *databaseCopyReader) Read(buffer []byte) (int, error) {
	if err := reader.ctx.Err(); err != nil {
		return 0, err
	}
	return reader.input.Read(buffer)
}

func saveBeforeUpgrade(path string) (string, error) {
	backups := filepath.Join(filepath.Dir(path), "backups")
	if err := core.PrivateDirectory(backups); err != nil {
		return "", err
	}
	preview, err := snapshotDatabase(path, backups)
	if err != nil {
		return "", err
	}
	defer preview.close()
	name := beforeUpgradePrefix + uuid.NewString()
	target := filepath.Join(backups, name)
	if err := os.Rename(filepath.Dir(preview.path), target); err != nil {
		return "", err
	}
	dropEarlierUpgradeCopies(backups, name)
	return filepath.Join(target, filepath.Base(path)), nil
}

const beforeUpgradePrefix = "node-before-upgrade-"

// dropEarlierUpgradeCopies keeps only the copy named kept. Each copy is a
// whole database, so one per update would grow without bound; the state
// before the latest update is the one a failed update returns to. A copy that
// cannot be removed stays for the next update to remove.
func dropEarlierUpgradeCopies(backups, kept string) {
	root, err := os.OpenRoot(backups)
	if err != nil {
		return
	}
	defer root.Close()
	directory, err := root.Open(".")
	if err != nil {
		return
	}
	entries, err := directory.ReadDir(-1)
	_ = directory.Close()
	if err != nil {
		return
	}
	for _, entry := range entries {
		if entry.IsDir() && entry.Name() != kept && strings.HasPrefix(entry.Name(), beforeUpgradePrefix) {
			_ = root.RemoveAll(entry.Name())
		}
	}
}

func snapshotUnavailable() error {
	return errors.New("database snapshot requires private regular files")
}
