package core

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// VerifyWorktreeMembership compares the common Git directories of retained roots.
func VerifyWorktreeMembership(ctx context.Context, root, destination string) error {
	if len(destination) > 4096 || strings.ContainsFunc(destination, unicode.IsControl) || !filepath.IsAbs(destination) {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "expected an absolute worktree directory"}
	}
	retained, err := OpenFiles(root, ManualFiles)
	if err != nil {
		return err
	}
	defer retained.Close()
	member, err := OpenFiles(destination, ManualFiles)
	if err != nil {
		return err
	}
	defer member.Close()
	rootBefore, err := os.Stat(root)
	if err != nil {
		return err
	}
	memberBefore, err := os.Stat(destination)
	if err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	common, err := gitCommon(root)
	if err != nil {
		return err
	}
	other, err := gitCommon(destination)
	if err != nil {
		return err
	}
	if common != other {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "worktree belongs to a different Git repository"}
	}
	for _, entry := range []struct {
		path   string
		before os.FileInfo
	}{{root, rootBefore}, {destination, memberBefore}} {
		current, err := OpenFiles(entry.path, ManualFiles)
		if err != nil {
			return err
		}
		_ = current.Close()
		after, err := os.Stat(entry.path)
		if err != nil {
			return err
		}
		if !os.SameFile(entry.before, after) {
			return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "worktree directory changed during inspection"}
		}
	}
	return nil
}

func gitCommon(root string) (string, error) {
	repository, err := gogit.PlainOpenWithOptions(root, &gogit.PlainOpenOptions{EnableDotGitCommonDir: true})
	if err != nil {
		return "", &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "Git repository is unavailable"}
	}
	storage, ok := repository.Storer.(*filesystem.Storage)
	if !ok {
		return "", &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "Git filesystem is unavailable"}
	}
	fs := storage.Filesystem()
	directory := fs.Root()
	file, err := fs.Open("commondir")
	if err == nil {
		data, readErr := io.ReadAll(file)
		closeErr := file.Close()
		if err := errors.Join(readErr, closeErr); err != nil {
			return "", err
		}
		common := strings.TrimSpace(string(data))
		if filepath.IsAbs(common) {
			directory = common
		} else {
			directory = filepath.Join(directory, common)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	return filepath.EvalSymlinks(directory)
}
