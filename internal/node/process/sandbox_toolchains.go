//go:build darwin || linux

package process

import (
	"io"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"
)

type toolchainCacheKey struct {
	search, home, profile string
}

type toolchainDirStamp struct {
	path    string
	exists  bool
	modTime time.Time
	size    int64
}

type toolchainCacheEntry struct {
	stamps   []toolchainDirStamp
	paths    []allowedPath
	links    map[string]string
	resolved string
}

var (
	toolchainCacheMu sync.Mutex
	toolchainCache   = map[toolchainCacheKey]toolchainCacheEntry{}
)

func stampToolchainDirs(entries []string) []toolchainDirStamp {
	stamps := make([]toolchainDirStamp, len(entries))
	for i, dir := range entries {
		info, err := os.Stat(dir)
		if err != nil || !info.IsDir() {
			stamps[i] = toolchainDirStamp{path: dir}
			continue
		}
		stamps[i] = toolchainDirStamp{path: dir, exists: true, modTime: info.ModTime(), size: info.Size()}
	}
	return stamps
}

func (policy *sandboxPolicy) configurePaths(search string) {
	delete(policy.variables, "PATH")
	if search == "" {
		return
	}
	rawEntries := filepath.SplitList(search)
	key := toolchainCacheKey{search: search, home: policy.home, profile: policy.profile}
	stamps := stampToolchainDirs(rawEntries)
	toolchainCacheMu.Lock()
	cached, ok := toolchainCache[key]
	toolchainCacheMu.Unlock()
	if ok && slices.Equal(cached.stamps, stamps) {
		for _, entry := range cached.paths {
			if !slices.ContainsFunc(policy.paths, func(existing allowedPath) bool { return within(existing.path, entry.path) }) {
				policy.paths = append(policy.paths, entry)
			}
		}
		maps.Copy(policy.links, cached.links)
		policy.variables["PATH"] = cached.resolved
		return
	}
	scratch := sandboxPolicy{variables: map[string]string{}, links: map[string]string{}, home: policy.home, profile: policy.profile}
	paths := slices.Clone(rawEntries)
	visited := map[string]bool{}
	for index, entry := range paths {
		directory := scratch.read(entry)
		if info, err := os.Stat(directory); err != nil || !info.IsDir() {
			continue
		}
		paths[index] = directory
		if systemToolchain(directory) {
			continue
		}
		visited[directory] = true
		scratch.libraries(directory)
		entries, err := os.ReadDir(directory)
		if err != nil {
			continue
		}
		for _, entry := range entries {
			scratch.executable(filepath.Join(directory, entry.Name()), search, visited)
		}
	}
	resolved := strings.Join(paths, string(os.PathListSeparator))
	toolchainCacheMu.Lock()
	if len(toolchainCache) >= 32 {
		clear(toolchainCache)
	}
	toolchainCache[key] = toolchainCacheEntry{
		stamps:   stamps,
		paths:    slices.Clone(scratch.paths),
		links:    maps.Clone(scratch.links),
		resolved: resolved,
	}
	toolchainCacheMu.Unlock()
	for _, entry := range scratch.paths {
		if !slices.ContainsFunc(policy.paths, func(existing allowedPath) bool { return within(existing.path, entry.path) }) {
			policy.paths = append(policy.paths, entry)
		}
	}
	maps.Copy(policy.links, scratch.links)
	policy.variables["PATH"] = resolved
}

func systemToolchain(path string) bool {
	for _, root := range []string{"/usr", "/bin", "/sbin", "/System", "/opt/homebrew"} {
		if within(root, path) {
			return true
		}
	}
	return false
}

func (policy *sandboxPolicy) executable(path, search string, visited map[string]bool) {
	resolved := policy.readable(path)
	if resolved == "" || visited[resolved] {
		return
	}
	info, err := os.Stat(resolved)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0111 == 0 {
		return
	}
	visited[resolved] = true
	policy.linkAliases(path)
	policy.read(resolved)
	directory := filepath.Dir(resolved)
	if !visited[directory] {
		visited[directory] = true
		policy.libraries(directory)
	}
	for ancestor := directory; ancestor != policy.home && filepath.Dir(ancestor) != ancestor; ancestor = filepath.Dir(ancestor) {
		if filepath.Base(ancestor) == "node_modules" {
			policy.read(ancestor)
			break
		}
		if regular(filepath.Join(ancestor, "pyvenv.cfg")) {
			for _, part := range []string{"bin", "lib", "lib64", "pyvenv.cfg"} {
				policy.read(filepath.Join(ancestor, part))
			}
			break
		}
	}
	file, err := os.Open(resolved)
	if err != nil {
		return
	}
	bytes, err := io.ReadAll(io.LimitReader(file, 4096))
	file.Close()
	if err != nil || !strings.HasPrefix(string(bytes), "#!") {
		return
	}
	line, _, _ := strings.Cut(string(bytes[2:]), "\n")
	words := strings.Fields(line)
	if len(words) == 0 {
		return
	}
	interpreter := words[0]
	if filepath.Base(interpreter) == "env" {
		interpreter = ""
		for _, word := range words[1:] {
			if !strings.HasPrefix(word, "-") && !strings.Contains(word, "=") {
				interpreter = findExecutable(word, search, directory)
				break
			}
		}
	}
	if interpreter != "" {
		policy.executable(interpreter, search, visited)
	}
}

func findExecutable(name, search, directory string) string {
	if strings.ContainsRune(name, filepath.Separator) {
		if !filepath.IsAbs(name) {
			name = filepath.Join(directory, name)
		}
		return name
	}
	for _, parent := range filepath.SplitList(search) {
		if !filepath.IsAbs(parent) {
			parent = filepath.Join(directory, parent)
		}
		path := filepath.Join(parent, name)
		if info, err := os.Stat(path); err == nil && info.Mode().IsRegular() && info.Mode().Perm()&0111 != 0 {
			return path
		}
	}
	return ""
}

func (policy *sandboxPolicy) libraries(directory string) {
	if filepath.Base(directory) != "bin" {
		return
	}
	prefix := filepath.Dir(directory)
	managed := regular(filepath.Join(prefix, "pyvenv.cfg"))
	if policy.home != "" {
		for _, relative := range []string{".nvm/versions/node", ".pyenv/versions", ".asdf/installs", ".local/share/mise/installs", ".local/share/uv/python", ".local/share/uv/tools", ".local/share/pipx/venvs", ".local/pipx/venvs", "Library/FlyEnv/env"} {
			managed = managed || within(filepath.Join(policy.home, relative), prefix)
		}
	}
	for _, relative := range []string{"lib", "lib64", "share"} {
		directory := filepath.Join(prefix, relative)
		if managed {
			policy.read(directory)
			continue
		}
		entries, err := os.ReadDir(directory)
		if err != nil {
			continue
		}
		for _, entry := range entries {
			suffix, ok := strings.CutPrefix(entry.Name(), "python")
			if !ok {
				suffix, ok = strings.CutPrefix(entry.Name(), "libpython")
			}
			if ok && len(suffix) > 0 && suffix[0] >= '0' && suffix[0] <= '9' {
				policy.read(filepath.Join(directory, entry.Name()))
			}
		}
	}
}

func (policy *sandboxPolicy) configureManagers(state string) error {
	if policy.home == "" {
		return nil
	}
	for _, manager := range []struct {
		relative, variable string
		parts              []string
	}{
		{".pyenv", "PYENV_ROOT", []string{"bin", "shims", "libexec", "plugins", "versions", "version"}},
		{".asdf", "ASDF_DATA_DIR", []string{"bin", "shims", "lib", "libexec", "plugins", "installs"}},
		{".volta", "VOLTA_HOME", []string{"bin", "tools"}},
		{".local/share/mise", "MISE_DATA_DIR", []string{"shims", "installs", "plugins"}},
	} {
		root := policy.readable(filepath.Join(policy.home, manager.relative))
		if info, err := os.Stat(root); err != nil || !info.IsDir() {
			continue
		}
		for _, part := range manager.parts {
			policy.read(filepath.Join(root, part))
		}
		policy.variables[manager.variable] = root
		if manager.relative == ".asdf" {
			policy.variables["ASDF_DIR"] = root
		}
		if manager.relative != ".local/share/mise" {
			if err := stateLink(state, root, filepath.Join("home", manager.relative)); err != nil {
				return err
			}
		}
	}
	for _, pair := range [][2]string{{".tool-versions", "home/.tool-versions"}, {".python-version", "home/.python-version"}, {".config/mise/config.toml", "config/mise/config.toml"}} {
		if source := policy.read(filepath.Join(policy.home, pair[0])); source != "" && regular(source) {
			if err := stateLink(state, source, pair[1]); err != nil {
				return err
			}
		}
	}
	return nil
}

func (policy *sandboxPolicy) linkAliases(path string) {
	if !filepath.IsAbs(path) {
		return
	}
	visited := map[string]bool{}
	for !visited[path] {
		visited[path] = true
		parts := strings.Split(strings.TrimPrefix(path, string(filepath.Separator)), string(filepath.Separator))
		prefix := string(filepath.Separator)
		found := false
		for index, part := range parts {
			prefix = filepath.Join(prefix, part)
			target, err := os.Readlink(prefix)
			if err != nil {
				continue
			}
			if prefix != "/bin" && prefix != "/sbin" && prefix != "/lib" && prefix != "/lib64" {
				policy.links[prefix] = target
			}
			if !filepath.IsAbs(target) {
				target = filepath.Join(filepath.Dir(prefix), target)
			}
			path = filepath.Join(append([]string{target}, parts[index+1:]...)...)
			found = true
			break
		}
		if !found {
			break
		}
	}
}
