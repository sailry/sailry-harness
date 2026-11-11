package core

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"strings"
	"unicode/utf8"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/go-git/gcfg"
	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const maxFileIgnoreBytes = 1024 * 1024

func loadFileIgnore(directory *os.Root, prefix, name string, control fileControl, report *FileWalkReport) fileIgnoreLayer {
	text, err := readFileIgnore(directory, name, control)
	if err != nil {
		report.notice(prefix, err.Error())
		return nil
	}
	if text == nil {
		return nil
	}
	var domain []string
	if prefix != "" {
		domain = strings.Split(prefix, "/")
	}
	var patterns fileIgnoreLayer
	for _, line := range strings.Split(*text, "\n") {
		if err := control.check(); err != nil {
			report.notice(prefix, err.Error())
			return nil
		}
		line = strings.TrimSuffix(line, "\r")
		if strings.HasPrefix(line, "#") || strings.TrimSpace(line) == "" {
			continue
		}
		if _, err := path.Match(strings.TrimPrefix(line, "!"), ""); err != nil {
			report.notice(prefix, fmt.Sprintf("invalid %s pattern", name))
			return nil
		}
		patterns = append(patterns, directFileIgnoreRule(line, domain))
	}
	return patterns
}

func directFileIgnoreRule(line string, domain []string) fileIgnoreRule {
	included := strings.HasPrefix(line, "!")
	if included {
		line = line[1:]
	}
	if !strings.HasSuffix(line, "\\ ") {
		line = strings.TrimRight(line, " ")
	}
	directoryOnly := strings.HasSuffix(line, "/")
	if directoryOnly {
		line = strings.TrimSuffix(line, "/")
	}
	if !strings.Contains(line, "/") {
		if included {
			line = "!" + line
		}
		return fileIgnoreRule{pattern: gitignore.ParsePattern(line, nil), directoriesOnly: directoryOnly, basename: true, domain: append([]string(nil), domain...)}
	}
	// Match only the entry itself, as the traversal already prunes ignored
	// parents. An impossible filename component anchors the library's matcher
	// at the end, preserving explicit access to an otherwise ignored scope.
	line += "/\x00"
	if included {
		line = "!" + line
	}
	return fileIgnoreRule{pattern: gitignore.ParsePattern(line, domain), directoriesOnly: directoryOnly}
}

func readFileIgnore(directory *os.Root, name string, control fileControl) (*string, error) {
	metadata, err := directory.Lstat(name)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fileIOFault(err)
	}
	if !metadata.Mode().IsRegular() {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, fmt.Sprintf("%s is not a regular file; its rules were not loaded", name))
	}
	if metadata.Size() > maxFileIgnoreBytes {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, fmt.Sprintf("%s exceeds the 1 MiB limit", name))
	}
	file, err := openRegularFile(directory, name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := readFileBytes(file, maxFileIgnoreBytes+1, control)
	if err != nil {
		return nil, err
	}
	if len(data) > maxFileIgnoreBytes {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, fmt.Sprintf("%s exceeds the 1 MiB limit", name))
	}
	if !utf8.Valid(data) {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, fmt.Sprintf("%s is not UTF-8 text", name))
	}
	text := string(data)
	return &text, nil
}

func (files *Files) baseIgnoreRules(control fileControl, report *FileWalkReport) []fileIgnoreLayer {
	gitdir, common, err := files.gitIgnoreDirectories(control)
	if err != nil {
		report.notice(".git", err.Error())
		return nil
	}
	if gitdir == "" {
		return nil
	}
	configuration := fileIgnoreConfiguration{control: control, active: make(map[string]bool), gitdir: filepath.ToSlash(gitdir), discovery: true}
	if file, err := os.Open(filepath.Join(gitdir, "HEAD")); err == nil {
		head, err := readFileBytes(file, maxFileIgnoreBytes, control)
		file.Close()
		if err == nil {
			configuration.branch, _ = strings.CutPrefix(strings.TrimSpace(string(head)), "ref: refs/heads/")
			if configuration.branch == strings.TrimSpace(string(head)) {
				configuration.branch = ""
			}
		}
	}
	sources := append(append([]string{}, files.globalConfigs...), filepath.Join(common, "config"))
	for _, name := range sources {
		_ = configuration.read(name)
	}
	if configuration.worktree {
		sources = append(sources, filepath.Join(gitdir, "config.worktree"))
		_ = configuration.read(sources[len(sources)-1])
	}
	configuration.discovery = false
	configuration.excludes = nil
	for _, name := range sources {
		if err := configuration.read(name); err != nil {
			report.notice("", fmt.Sprintf("could not load Git ignore configuration: %v", err))
		}
	}
	var layers []fileIgnoreLayer
	if configuration.excludes != nil {
		if *configuration.excludes != "" {
			configured, err := expandFileIgnorePath(*configuration.excludes, files.path)
			if err != nil {
				report.notice("", err.Error())
			} else {
				layers = append(layers, externalFileIgnore(configured, control, report))
			}
		}
	} else if files.defaultIgnore != "" {
		layers = append(layers, externalFileIgnore(files.defaultIgnore, control, report))
	}
	layers = append(layers, externalFileIgnore(filepath.Join(common, "info", "exclude"), control, report))
	return layers
}

func fileGitConfigSources() ([]string, string) {
	var sources []string
	if os.Getenv("GIT_CONFIG_NOSYSTEM") == "" {
		if system, set := os.LookupEnv("GIT_CONFIG_SYSTEM"); set {
			if system != "" {
				sources = append(sources, system)
			}
		} else if runtime.GOOS != "windows" {
			sources = append(sources, "/etc/gitconfig")
		} else if programData := os.Getenv("PROGRAMDATA"); programData != "" {
			sources = append(sources, filepath.Join(programData, "Git", "config"))
		}
	}
	home, err := os.UserHomeDir()
	xdg := os.Getenv("XDG_CONFIG_HOME")
	if xdg == "" && err == nil {
		xdg = filepath.Join(home, ".config")
	}
	if global, set := os.LookupEnv("GIT_CONFIG_GLOBAL"); set {
		if global != "" {
			sources = append(sources, global)
		}
	} else {
		if xdg != "" {
			sources = append(sources, filepath.Join(xdg, "git", "config"))
		}
		if err == nil {
			sources = append(sources, filepath.Join(home, ".gitconfig"))
		}
	}
	if xdg != "" {
		return sources, filepath.Join(xdg, "git", "ignore")
	}
	return sources, ""
}

func externalFileIgnore(filename string, control fileControl, report *FileWalkReport) fileIgnoreLayer {
	directory, err := acquireFileRoot(filepath.Dir(filename))
	if isFileFault(err, protocol.ErrorCodeNotFound) {
		return nil
	}
	if err != nil {
		report.notice("", err.Error())
		return nil
	}
	defer directory.Close()
	return loadFileIgnore(directory, "", filepath.Base(filename), control, report)
}

func (files *Files) gitIgnoreDirectories(control fileControl) (string, string, error) {
	if err := control.check(); err != nil {
		return "", "", err
	}
	metadata, err := files.root.Lstat(".git")
	if errors.Is(err, fs.ErrNotExist) {
		return "", "", nil
	}
	if err != nil {
		return "", "", fileIOFault(err)
	}
	if metadata.Mode()&os.ModeSymlink != 0 {
		return "", "", fileFault(protocol.ErrorCodeInvalidRequest, "Git metadata cannot be a symbolic link")
	}
	gitdir := filepath.Join(files.path, ".git")
	if !metadata.IsDir() {
		file, err := openRegularFile(files.root, ".git")
		if err != nil {
			return "", "", err
		}
		data, err := readFileBytes(file, maxFileIgnoreBytes, control)
		file.Close()
		if err != nil {
			return "", "", err
		}
		value, valid := strings.CutPrefix(strings.TrimSpace(string(data)), "gitdir:")
		if !valid || strings.TrimSpace(value) == "" {
			return "", "", fileFault(protocol.ErrorCodeInvalidRequest, "could not open Git ignore configuration: invalid gitdir")
		}
		gitdir, err = expandFileIgnorePath(strings.TrimSpace(value), files.path)
		if err != nil {
			return "", "", err
		}
	}
	root, err := acquireFileRoot(gitdir)
	if err != nil {
		return "", "", err
	}
	defer root.Close()
	common := gitdir
	file, err := openRegularFile(root, "commondir")
	if err == nil {
		data, readErr := readFileBytes(file, maxFileIgnoreBytes, control)
		file.Close()
		if readErr != nil {
			return "", "", readErr
		}
		common, err = expandFileIgnorePath(strings.TrimSpace(string(data)), gitdir)
		if err != nil {
			return "", "", err
		}
	} else if !isFileFault(err, protocol.ErrorCodeNotFound) {
		return "", "", err
	}
	return gitdir, common, nil
}

type fileIgnoreConfiguration struct {
	excludes   *string
	worktree   bool
	control    fileControl
	active     map[string]bool
	gitdir     string
	branch     string
	remoteURLs []string
	discovery  bool
}

func (configuration *fileIgnoreConfiguration) read(filename string) error {
	if err := configuration.control.check(); err != nil {
		return err
	}
	filename = filepath.Clean(filename)
	if configuration.active[filename] {
		return fileFault(protocol.ErrorCodeInvalidRequest, "Git configuration includes a cycle")
	}
	file, err := os.Open(filename)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	defer file.Close()
	configuration.active[filename] = true
	defer delete(configuration.active, filename)
	return gcfg.ReadWithCallback(file, func(section, subsection, key, value string, _ bool) error {
		if err := configuration.control.check(); err != nil {
			return err
		}
		switch {
		case strings.EqualFold(section, "core") && strings.EqualFold(key, "excludesfile"):
			copy := value
			configuration.excludes = &copy
		case strings.EqualFold(section, "extensions") && strings.EqualFold(key, "worktreeconfig"):
			configuration.worktree = strings.EqualFold(value, "true") || value == "1" || strings.EqualFold(value, "yes") || strings.EqualFold(value, "on")
		case strings.EqualFold(section, "include") && strings.EqualFold(key, "path"):
			included, err := expandFileIgnorePath(value, filepath.Dir(filename))
			if err != nil {
				return err
			}
			return configuration.read(included)
		case strings.EqualFold(section, "remote") && strings.EqualFold(key, "url") && configuration.discovery:
			configuration.remoteURLs = append(configuration.remoteURLs, value)
		case strings.EqualFold(section, "includeif") && strings.EqualFold(key, "path"):
			matches, err := configuration.includeCondition(subsection, filename)
			if err != nil {
				return err
			}
			if matches {
				included, err := expandFileIgnorePath(value, filepath.Dir(filename))
				if err != nil {
					return err
				}
				return configuration.read(included)
			}
		}
		return nil
	})
}

func (configuration *fileIgnoreConfiguration) includeCondition(condition, origin string) (bool, error) {
	switch {
	case strings.HasPrefix(condition, "gitdir:") || strings.HasPrefix(condition, "gitdir/i:"):
		fold := strings.HasPrefix(condition, "gitdir/i:")
		pattern := strings.TrimPrefix(strings.TrimPrefix(condition, "gitdir:"), "gitdir/i:")
		if strings.HasPrefix(pattern, "~/") {
			expanded, err := expandFileIgnorePath(pattern, "")
			if err != nil {
				return false, err
			}
			pattern = filepath.ToSlash(expanded)
		} else if strings.HasPrefix(pattern, "./") {
			pattern = filepath.ToSlash(filepath.Join(filepath.Dir(origin), pattern))
		} else if !filepath.IsAbs(pattern) {
			pattern = "**/" + pattern
		}
		if strings.HasSuffix(pattern, "/") {
			pattern += "**"
		}
		gitdir := strings.TrimSuffix(configuration.gitdir, "/")
		if fold {
			pattern, gitdir = strings.ToLower(pattern), strings.ToLower(gitdir)
		}
		return doublestar.Match(pattern, gitdir)
	case strings.HasPrefix(condition, "onbranch:"):
		if configuration.branch == "" {
			return false, nil
		}
		pattern := strings.TrimPrefix(condition, "onbranch:")
		if strings.HasSuffix(pattern, "/") {
			pattern += "**"
		}
		return doublestar.Match(pattern, configuration.branch)
	case strings.HasPrefix(condition, "hasconfig:remote.*.url:"):
		if configuration.discovery {
			return false, nil
		}
		pattern := strings.TrimPrefix(condition, "hasconfig:remote.*.url:")
		for _, url := range configuration.remoteURLs {
			matched, err := doublestar.Match(pattern, url)
			if err != nil {
				return false, err
			}
			if matched {
				return true, nil
			}
		}
	}
	return false, nil
}

func expandFileIgnorePath(value, base string) (string, error) {
	if strings.HasPrefix(value, "~/") || value == "~" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		if value == "~" {
			value = home
		} else {
			value = filepath.Join(home, strings.TrimPrefix(value, "~/"))
		}
	}
	if filepath.IsAbs(value) {
		return value, nil
	}
	return filepath.Join(base, value), nil
}
