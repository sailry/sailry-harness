package backup

import (
	"context"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TargetPath accepts a new or empty absolute directory for a restored profile.
// A non-empty profile also refuses any overlap with the running profile.
func TargetPath(value, profile string) (string, error) {
	if !filepath.IsAbs(value) || filepath.Clean(value) == filepath.Dir(filepath.Clean(value)) || traverses(value) {
		return "", invalid("restore requires a new absolute profile directory")
	}
	path := filepath.Clean(value)
	info, err := os.Lstat(path)
	switch {
	case errors.Is(err, fs.ErrNotExist):
	case err != nil:
		return "", ioFault(err)
	case !info.IsDir():
		return "", fault(protocol.ErrorCodeConflict, "restore destination must be an empty directory")
	default:
		directory, err := os.Open(path)
		if err != nil {
			return "", ioFault(err)
		}
		names, err := directory.Readdirnames(1)
		_ = directory.Close()
		if err != nil && !errors.Is(err, io.EOF) {
			return "", ioFault(err)
		}
		if len(names) != 0 {
			return "", fault(protocol.ErrorCodeConflict, "restore destination must be empty")
		}
	}
	if profile != "" {
		overlap, err := overlapsProfile(path, profile)
		if err != nil {
			return "", err
		}
		if overlap {
			return "", fault(protocol.ErrorCodePermissionDenied, "restore destination overlaps the active profile")
		}
	}
	return path, nil
}

// LocalTarget refuses a local backup directory inside or around the profile.
func LocalTarget(value, profile string) error {
	overlap, err := overlapsProfile(value, profile)
	if err != nil {
		return err
	}
	if overlap {
		return fault(protocol.ErrorCodePermissionDenied, "local backup destination overlaps the active profile")
	}
	return nil
}

// overlapsProfile compares physical locations, so a symbolic link or an
// alternative spelling of a parent cannot place a directory in the profile.
func overlapsProfile(value, profile string) (bool, error) {
	path, err := physical(value)
	if err != nil {
		return false, err
	}
	active, err := physical(profile)
	if err != nil {
		return false, err
	}
	return within(path, active) || within(active, path), nil
}

// physical resolves the existing part of a path and keeps the missing rest.
func physical(value string) (string, error) {
	existing := filepath.Clean(value)
	for {
		_, err := os.Lstat(existing)
		if err == nil {
			break
		}
		if !errors.Is(err, fs.ErrNotExist) {
			return "", ioFault(err)
		}
		parent := filepath.Dir(existing)
		if parent == existing {
			return "", invalid("backup parent is missing")
		}
		existing = parent
	}
	resolved, err := filepath.EvalSymlinks(existing)
	if err != nil {
		return "", ioFault(err)
	}
	remainder, err := filepath.Rel(existing, filepath.Clean(value))
	if err != nil {
		return "", ioFault(err)
	}
	return filepath.Join(resolved, remainder), nil
}

func traverses(path string) bool {
	for _, part := range strings.Split(filepath.ToSlash(path), "/") {
		if part == "." || part == ".." {
			return true
		}
	}
	return false
}

func within(path, directory string) bool {
	relative, err := filepath.Rel(directory, path)
	return err == nil && filepath.IsLocal(relative)
}

// Staged is a verified archive unpacked in the Node's private transfer
// directory, where its databases are validated before publication.
type Staged struct {
	Directory string
	Manifest  Manifest
	Record    protocol.BackupRecord
}

func (staged *Staged) Close() error { return os.RemoveAll(staged.Directory) }

// Stage unpacks a decrypted archive stream into a new directory in temporary.
// A nil known record describes an uploaded archive of the given sealed size
// and checksum.
func Stage(ctx context.Context, archive io.Reader, temporary string, known *protocol.BackupRecord, id string, size uint64, sum string) (*Staged, error) {
	directory, err := os.MkdirTemp(temporary, "restore-")
	if err != nil {
		return nil, ioFault(err)
	}
	manifest, err := Unpack(ctx, archive, directory)
	if err != nil {
		_ = os.RemoveAll(directory)
		return nil, err
	}
	record := protocol.BackupRecord{
		ID: id, Node: manifest.Node, CreatedMs: uint64(time.Now().UnixMilli()),
		SizeBytes: size, Files: uint32(len(manifest.Files)), Sha256: sum,
	}
	if known != nil {
		record = *known
	}
	if manifest.Node != record.Node || len(manifest.Files) != int(record.Files) {
		_ = os.RemoveAll(directory)
		return nil, invalid("restored profile differs from remote metadata")
	}
	return &Staged{Directory: directory, Manifest: manifest, Record: record}, nil
}

// Finish publishes validated staging as the target directory with one rename.
// retained lists staging-relative files outside the manifest that the restored
// profile keeps, such as the pre-upgrade database copy.
func Finish(ctx context.Context, staged *Staged, target string, retained []string) error {
	target, err := TargetPath(target, "")
	if err != nil {
		return err
	}
	if err := staged.Manifest.RefreshDatabases(ctx, staged.Directory); err != nil {
		return err
	}
	parent, err := os.OpenRoot(filepath.Dir(target))
	if err != nil {
		return ioFault(err)
	}
	defer parent.Close()
	identity, err := parent.Stat(".")
	if err != nil {
		return ioFault(err)
	}
	name := ".sailry-restore-" + uuid.NewString()
	staging := filepath.Join(filepath.Dir(target), name)
	// This directory becomes the profile itself. Creating only its storage
	// child would leave the profile root with an inherited Windows DACL.
	if err := core.PrivateDirectory(staging); err != nil {
		return ioFault(err)
	}
	published := false
	defer func() {
		if !published {
			_ = parent.RemoveAll(name)
		}
	}()
	if err := core.PrivateDirectory(filepath.Join(staging, "storage")); err != nil {
		return ioFault(err)
	}
	destination, err := parent.OpenRoot(name)
	if err != nil {
		return ioFault(err)
	}
	err = Transfer(ctx, staged.Directory, destination, staged.Manifest)
	for _, relative := range retained {
		if err != nil {
			break
		}
		err = retain(ctx, staged.Directory, destination, relative)
	}
	_ = destination.Close()
	if err != nil {
		return err
	}
	if err := cancelled(ctx); err != nil {
		return err
	}
	if _, err := TargetPath(target, ""); err != nil {
		return err
	}
	current, err := os.Stat(filepath.Dir(target))
	if err != nil {
		return ioFault(err)
	}
	if !os.SameFile(identity, current) {
		return fault(protocol.ErrorCodeRevisionConflict, "restore parent changed")
	}
	if err := parent.Remove(filepath.Base(target)); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return ioFault(err)
	}
	if err := parent.Rename(name, filepath.Base(target)); err != nil {
		return ioFault(err)
	}
	published = true
	return nil
}

func retain(ctx context.Context, source string, destination *os.Root, relative string) error {
	input, err := os.Open(filepath.Join(source, filepath.FromSlash(relative)))
	if err != nil {
		return ioFault(err)
	}
	defer input.Close()
	info, err := input.Stat()
	if err != nil {
		return ioFault(err)
	}
	output, err := create(destination, relative)
	if err != nil {
		return err
	}
	_, _, err = copyHash(ctx, input, output, uint64(info.Size()))
	if err == nil {
		err = wrapIO(output.Sync())
	}
	if closeErr := output.Close(); err == nil {
		err = wrapIO(closeErr)
	}
	return err
}
