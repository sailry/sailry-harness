package node

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func legacyProfileLinkedGit(path string) error {
	return fmt.Errorf("legacy profile contains a linked Git checkout at %q; use Git to safely move or finish linked worktrees outside the profile before retrying", path)
}

func checkLegacyProfileGit(root *os.Root, relative string, linked map[string]string) error {
	info, err := root.Lstat(filepath.Join(relative, ".git"))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if info.Mode().IsRegular() {
		admin, err := visitLegacyProfileRegistration(root, relative, root.Name(), "")
		if err != nil {
			return fmt.Errorf("linked Git checkout at %q: %w", filepath.Join(root.Name(), relative), err)
		}
		linked[relative] = admin
		return nil
	}
	if !info.IsDir() {
		return legacyProfileLinkedGit(filepath.Join(root.Name(), relative))
	}
	linkedPrimary, err := legacyGitDirectoryHasEntries(root, filepath.Join(relative, ".git", "worktrees"))
	if err == nil && linkedPrimary {
		return legacyProfileLinkedGit(filepath.Join(root.Name(), relative))
	}
	return err
}

func legacyGitDirectoryHasEntries(root *os.Root, name string) (bool, error) {
	directory, err := root.Open(name)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	entries, readErr := directory.ReadDir(1)
	if errors.Is(readErr, io.EOF) {
		readErr = nil
	}
	return len(entries) != 0, errors.Join(readErr, directory.Close())
}

// Inspect only known managed roots and registered paths, never walk user trees
// or inspect project contents.
func checkLegacyProfileWorktrees(profile string, linked map[string]string) error {
	root, err := os.OpenRoot(profile)
	if err != nil {
		return err
	}
	defer root.Close()
	if err := checkLegacyProfileGit(root, ".", linked); err != nil {
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
					if err := checkLegacyProfileGit(root, filepath.Join(name, entry.Name()), linked); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}

func checkLegacyProfileRegisteredGit(ctx context.Context, db *sql.Conn, profile string, linked map[string]string, aliases ...string) error {
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
			if err := checkLegacyProfileGit(root, relative, linked); err != nil {
				return err
			}
			break
		}
	}
	return rows.Err()
}

func repairLegacyProfileWorktrees(legacy, target string, linked map[string]string) error {
	if len(linked) == 0 {
		return nil
	}
	root, err := os.OpenRoot(target)
	if err != nil {
		return err
	}
	defer root.Close()
	for relative, admin := range linked {
		if !filepath.IsLocal(relative) || filepath.Clean(relative) != relative || admin == "" {
			return fmt.Errorf("invalid linked Git checkout in migration journal: %q", relative)
		}
		if _, err := visitLegacyProfileRegistration(root, relative, legacy, admin); err != nil {
			return fmt.Errorf("repair linked Git checkout at %q: %w", filepath.Join(target, relative), err)
		}
	}
	return nil
}

// Git stores an absolute pointer in the checkout and a reciprocal gitdir file
// under its owner's .git/worktrees directory. Only that backlink needs to
// change. Recheck both ends after publication, including on journal recovery.
func visitLegacyProfileRegistration(root *os.Root, relative, legacy, expectedAdmin string) (string, error) {
	name := filepath.Join(relative, ".git")
	gitfile, _, err := readLegacyGitLine(root, name)
	if err != nil {
		return "", err
	}
	pointer, ok := strings.CutPrefix(gitfile, "gitdir: ")
	if !ok || !filepath.IsAbs(pointer) {
		return "", fmt.Errorf("expected an absolute Git worktree registration in %q", filepath.Join(root.Name(), name))
	}
	admin, err := filepath.EvalSymlinks(pointer)
	if err != nil {
		return "", fmt.Errorf("open external Git worktree registration %q: %w", pointer, err)
	}
	worktrees := filepath.Dir(admin)
	common := filepath.Dir(worktrees)
	if filepath.Base(worktrees) != "worktrees" || filepath.Base(common) != ".git" {
		return "", fmt.Errorf("unsupported Git worktree registration %q", admin)
	}
	for _, profile := range []string{legacy, root.Name()} {
		for _, candidate := range []string{admin, filepath.Clean(pointer)} {
			if inside, err := filepath.Rel(profile, candidate); err == nil && filepath.IsLocal(inside) {
				return "", fmt.Errorf("Git worktree registration %q is inside the moving profile", candidate)
			}
		}
	}
	if expectedAdmin != "" && expectedAdmin != admin {
		return "", fmt.Errorf("Git worktree registration changed from %q to %q", expectedAdmin, admin)
	}

	directory, err := os.OpenRoot(admin)
	if err != nil {
		return "", err
	}
	defer directory.Close()
	// Initialized submodules keep their own core.worktree paths under modules.
	if nested, err := legacyGitDirectoryHasEntries(directory, "modules"); err != nil {
		return "", err
	} else if nested {
		return "", errors.New("the checkout has initialized submodules; relocate it with its submodules before retrying")
	}
	shared, _, err := readLegacyGitLine(directory, "commondir")
	if err != nil {
		return "", err
	}
	shared = filepath.FromSlash(shared)
	if !filepath.IsAbs(shared) {
		shared = admin + string(filepath.Separator) + shared
	}
	owner, err := os.Stat(common)
	if err != nil {
		return "", err
	}
	actual, err := os.Stat(shared)
	if err != nil {
		return "", err
	}
	if !os.SameFile(owner, actual) {
		return "", fmt.Errorf("Git worktree registration %q belongs to a different common directory", admin)
	}
	backlink, info, err := readLegacyGitLine(directory, "gitdir")
	if err != nil {
		return "", err
	}
	old, current := filepath.Join(legacy, name), filepath.Join(root.Name(), name)
	switch filepath.FromSlash(backlink) {
	case old:
		if expectedAdmin != "" && current != old {
			if err := replaceLegacyGitBacklink(directory, backlink, info, current); err != nil {
				return "", err
			}
		}
	case current:
		// The previous attempt installed the backlink before recording completion.
	default:
		return "", fmt.Errorf("Git worktree registration %q points to %q, expected %q", admin, backlink, old)
	}
	if expectedAdmin != "" {
		if err := syncLegacyProfileDirectory(admin); err != nil {
			return "", err
		}
	}
	return admin, nil
}

// These are Git's single-line path files. Read through the retained directory,
// and require the opened regular file to match what was inspected.
func readLegacyGitLine(root *os.Root, name string) (string, os.FileInfo, error) {
	info, err := root.Lstat(name)
	if err != nil {
		return "", nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > 64*1024 {
		return "", nil, fmt.Errorf("invalid Git registration file %q", filepath.Join(root.Name(), name))
	}
	f, err := root.Open(name)
	if err != nil {
		return "", nil, err
	}
	defer f.Close()
	opened, err := f.Stat()
	if err != nil {
		return "", nil, err
	}
	if !os.SameFile(info, opened) {
		return "", nil, fmt.Errorf("Git registration file %q changed", name)
	}
	body, err := io.ReadAll(io.LimitReader(f, info.Size()+1))
	if err != nil {
		return "", nil, err
	}
	line := strings.TrimSuffix(strings.TrimSuffix(string(body), "\n"), "\r")
	if int64(len(body)) != info.Size() || line == "" || strings.ContainsAny(line, "\x00\r\n") {
		return "", nil, fmt.Errorf("invalid Git registration file %q", filepath.Join(root.Name(), name))
	}
	return line, info, nil
}

func replaceLegacyGitBacklink(admin *os.Root, before string, info os.FileInfo, target string) error {
	name := ".sailry-gitdir-" + uuid.NewString()
	file, err := admin.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, info.Mode().Perm())
	if err != nil {
		return err
	}
	defer admin.Remove(name)
	_, writeErr := file.WriteString(filepath.ToSlash(target) + "\n")
	if err := errors.Join(writeErr, file.Chmod(info.Mode().Perm()), file.Sync(), file.Close()); err != nil {
		return err
	}
	current, currentInfo, err := readLegacyGitLine(admin, "gitdir")
	if err != nil {
		return err
	}
	if before != current || !os.SameFile(info, currentInfo) {
		return fmt.Errorf("Git worktree registration %q changed during migration", admin.Name())
	}
	return admin.Rename(name, "gitdir")
}
