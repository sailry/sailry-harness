package node

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func legacyProfileLinkedGit(path string) error {
	return fmt.Errorf("legacy profile contains a linked Git checkout at %q; use Git to safely move or finish linked worktrees outside the profile before retrying; neither the checkout nor its external repository was moved or repaired", path)
}

func checkLegacyProfileGit(root *os.Root, relative string) error {
	info, err := root.Lstat(filepath.Join(relative, ".git"))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return legacyProfileLinkedGit(filepath.Join(root.Name(), relative))
	}
	worktrees, err := root.Open(filepath.Join(relative, ".git", "worktrees"))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	entries, readErr := worktrees.ReadDir(1)
	if errors.Is(readErr, io.EOF) {
		readErr = nil
	}
	if err := errors.Join(readErr, worktrees.Close()); err != nil {
		return err
	}
	if len(entries) != 0 {
		return legacyProfileLinkedGit(filepath.Join(root.Name(), relative))
	}
	return nil
}

// Inspect only known managed roots and registered paths, never walk user trees
// or follow a .git pointer into an external repository.
func checkLegacyProfileWorktrees(profile string) error {
	root, err := os.OpenRoot(profile)
	if err != nil {
		return err
	}
	defer root.Close()
	if err := checkLegacyProfileGit(root, "."); err != nil {
		return err
	}
	parents := []string{"."}
	users, err := root.Open("users")
	if err == nil {
		entries, readErr := users.ReadDir(-1)
		err = errors.Join(readErr, users.Close())
		if err != nil {
			return err
		}
		for _, entry := range entries {
			if _, err := protocol.ParseUserID(entry.Name()); err == nil {
				parents = append(parents, filepath.Join("users", entry.Name()))
			}
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	for _, parent := range parents {
		for _, suffix := range []string{"worktrees", filepath.Join(".sailry", "worktrees")} {
			name := filepath.Join(parent, suffix)
			info, err := root.Lstat(name)
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return err
			}
			if !info.IsDir() {
				return legacyProfileLinkedGit(filepath.Join(profile, name))
			}
			directory, err := root.Open(name)
			if err != nil {
				return err
			}
			entries, readErr := directory.ReadDir(-1)
			if err := errors.Join(readErr, directory.Close()); err != nil {
				return err
			}
			for _, entry := range entries {
				if entry.Type()&os.ModeSymlink != 0 {
					return legacyProfileLinkedGit(filepath.Join(profile, name, entry.Name()))
				}
				if entry.IsDir() {
					if err := checkLegacyProfileGit(root, filepath.Join(name, entry.Name())); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}

func checkLegacyProfileRegisteredGit(ctx context.Context, db *sql.Conn, profile string, aliases ...string) error {
	root, err := os.OpenRoot(profile)
	if err != nil {
		return err
	}
	defer root.Close()
	rows, err := db.QueryContext(ctx, "SELECT path FROM projects UNION SELECT path FROM worktrees")
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var path string
		if err := rows.Scan(&path); err != nil {
			return err
		}
		for _, original := range append([]string{profile}, aliases...) {
			relative, err := filepath.Rel(original, path)
			if err != nil || relative != "." && !filepath.IsLocal(relative) {
				continue
			}
			if err := checkLegacyProfileGit(root, relative); err != nil {
				return err
			}
			break
		}
	}
	return rows.Err()
}
