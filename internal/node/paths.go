package node

import (
	"context"
	"database/sql"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) prepare(command protocol.Command) (store.PreparedPaths, error) {
	if draft, ok := command.Data.(*protocol.ProjectsDraft); ok && command.Kind == "create_project" && draft.Source.Kind == "clone" {
		if err := store.ValidateProjectDraft(*draft); err != nil {
			return store.PreparedPaths{}, err
		}
		source := draft.Source.Data.(*protocol.ProjectsSourceClonePayload)
		canonical, err := node.git.Clone(node.ctx, source.Url, draft.Path, source.Branch)
		return store.PreparedPaths{Path: canonical}, err
	}
	prepared, err := store.PreparePaths(command)
	if err != nil {
		return prepared, err
	}
	var project protocol.ProjectID
	switch input := command.Data.(type) {
	case *protocol.CommandRegisterWorktreePayload:
		project = input.Project
	case *protocol.CommandUpdateProjectPayload:
		if prepared.Path == input.Expected.Path {
			return prepared, nil
		}
		project = input.Expected.ID
	default:
		return prepared, nil
	}
	var root string
	var trees []protocol.Worktree
	err = node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		if err := db.QueryRowContext(node.ctx, "SELECT path FROM projects WHERE id=?1", project.String()).Scan(&root); err != nil {
			return err
		}
		var err error
		trees, err = store.ReadWorktrees(node.ctx, db)
		return err
	})
	if errors.Is(err, sql.ErrNoRows) {
		return prepared, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "project does not exist on this Node"}
	}
	if err != nil {
		return prepared, err
	}
	if command.Kind == "register_worktree" {
		return prepared, verifyGitMembership(node.ctx, root, prepared.Path)
	}
	for _, tree := range trees {
		if tree.Project != nil && *tree.Project == project && !tree.Main {
			if err := verifyGitMembership(node.ctx, prepared.Path, tree.Path); err != nil {
				return prepared, err
			}
		}
	}
	return prepared, nil
}

// go-git opens linked worktrees; retained roots reject replaced acquisition paths.
func verifyGitMembership(ctx context.Context, root, destination string) error {
	if len(destination) > 4096 || strings.ContainsFunc(destination, unicode.IsControl) || !filepath.IsAbs(destination) {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "expected an absolute worktree directory"}
	}
	retained, err := core.OpenFiles(root, core.ManualFiles)
	if err != nil {
		return err
	}
	defer retained.Close()
	member, err := core.OpenFiles(destination, core.ManualFiles)
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
		current, err := core.OpenFiles(entry.path, core.ManualFiles)
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
	repository, err := git.PlainOpenWithOptions(root, &git.PlainOpenOptions{EnableDotGitCommonDir: true})
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
