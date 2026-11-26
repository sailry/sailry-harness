package updater

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

// A portable app can share its directory with user files. Only the incoming
// payload and the old build's package inventory are owned by the updater.
func applyWindowsFiles(incoming, installed, backup string) (result error) {
	source, err := os.OpenRoot(incoming)
	if err != nil {
		return err
	}
	defer source.Close()
	target, err := os.OpenRoot(installed)
	if err != nil {
		return err
	}
	defer target.Close()
	var previous struct {
		Files []string `json:"files"`
	}
	metadata, err := target.ReadFile("build.json")
	if err != nil {
		return err
	}
	if err := json.Unmarshal(metadata, &previous); err != nil {
		return err
	}
	// The package preserves the installer's own uninstaller, shortcuts and
	// registration. Windows treats case variants as the same path.
	owned := make(map[string]bool, len(previous.Files))
	for _, name := range previous.Files {
		owned[strings.ToLower(filepath.FromSlash(name))] = true
	}
	seen := map[string]bool{}
	type replacement struct {
		name               string
		incoming, original bool
	}
	var files []replacement
	add := func(name string, incoming bool) error {
		if !filepath.IsLocal(name) || filepath.Clean(name) != name || name == "." || strings.EqualFold(name, "Uninstall.exe") {
			return fmt.Errorf("invalid Windows package file %q", name)
		}
		key := strings.ToLower(name)
		if seen[key] {
			return nil
		}
		seen[key] = true
		// Do not traverse junctions or links in the installed tree.
		for parent := filepath.Dir(name); parent != "."; parent = filepath.Dir(parent) {
			info, err := target.Lstat(parent)
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return err
			}
			if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				return fmt.Errorf("package directory %q is not a directory", parent)
			}
		}
		info, err := target.Lstat(name)
		exists := err == nil
		if exists && !info.Mode().IsRegular() {
			return fmt.Errorf("package file %q is not a regular file", name)
		}
		if exists && incoming && !owned[key] {
			return fmt.Errorf("update file %q conflicts with an existing non-package file", name)
		}
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		if incoming || exists {
			files = append(files, replacement{name, incoming, exists})
		}
		return nil
	}
	err = fs.WalkDir(source.FS(), ".", func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		if !entry.Type().IsRegular() {
			return fmt.Errorf("invalid Windows package entry %q", path)
		}
		return add(filepath.FromSlash(path), true)
	})
	if err != nil {
		return err
	}
	for _, path := range previous.Files {
		if err := add(filepath.FromSlash(path), false); err != nil {
			return err
		}
	}

	// Keep every original before modifying the first installed file. The
	// receipt keeps this backup until the successor confirms its version.
	for _, file := range files {
		if file.original {
			saved := filepath.Join(backup, file.name)
			if err := os.MkdirAll(filepath.Dir(saved), 0o700); err != nil {
				return err
			}
			if err := copyTree(filepath.Join(installed, file.name), saved); err != nil {
				return err
			}
		}
	}
	var applied []replacement
	defer func() {
		if result == nil {
			return
		}
		for _, file := range slices.Backward(applied) {
			var err error
			if file.original {
				err = os.Rename(filepath.Join(backup, file.name), filepath.Join(installed, file.name))
			} else {
				err = target.Remove(file.name)
			}
			if err != nil {
				result = failure("updates_install_uncertain", errors.Join(result, err).Error())
			}
		}
	}()
	for _, file := range files {
		if file.incoming {
			if err := target.MkdirAll(filepath.Dir(file.name), 0o755); err != nil {
				return err
			}
			if err := os.Rename(filepath.Join(incoming, file.name), filepath.Join(installed, file.name)); err != nil {
				return err
			}
		} else if err := target.Remove(file.name); err != nil {
			return err
		}
		applied = append(applied, file)
	}
	return nil
}
