package process

import (
	"context"
	"maps"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Environment freezes the Node's startup shell environment, outside tools.
type Environment struct {
	values       map[string]string
	terminal     map[string]string
	scoped       map[string]string
	unrestricted map[string]string
}

var baseVariables = []string{"HOME", "USERPROFILE", "PATH", "PATHEXT", "LANG", "LC_ALL", "LC_CTYPE", "SystemRoot", "SystemDrive", "COMSPEC", "TEMP", "TMP", "TMPDIR"}

// NewEnvironment never starts a shell, so deterministic tests can use fixtures.
func NewEnvironment(values []string) Environment {
	environment := Environment{values: map[string]string{}, terminal: map[string]string{}, scoped: map[string]string{}, unrestricted: map[string]string{}}
	for _, entry := range values {
		key, value, ok := strings.Cut(entry, "=")
		if !ok || key == "" {
			continue
		}
		if !secretVariable(key) {
			environment.unrestricted[key] = value
			if scopedVariable(key) {
				environment.scoped[key] = value
			}
		}
		if runtime.GOOS == "darwin" && key == "DEVELOPER_DIR" {
			if path := developerDirectory(value); path != "" {
				environment.values[key], environment.scoped[key], environment.terminal[key] = path, path, path
			}
			continue
		}
		canonical := ""
		for _, name := range baseVariables {
			if key == name || runtime.GOOS == "windows" && strings.EqualFold(key, name) {
				canonical = name
				break
			}
		}
		if canonical != "" {
			environment.values[canonical], environment.scoped[canonical], environment.terminal[canonical] = value, value, value
		} else if terminalVariable(key) {
			environment.terminal[key] = value
		}
	}
	return environment
}

// CaptureEnvironment runs the source's bounded login capture once at Node start.
// Tool calls reuse this snapshot and never rerun a user's shell profiles.
func CaptureEnvironment(ctx context.Context) Environment {
	environment := NewEnvironment(os.Environ())
	return captureLogin(ctx, environment)
}

func (environment Environment) TerminalValues() []string {
	return environmentValues(environment.terminal)
}

func (environment Environment) Shell() string {
	if runtime.GOOS == "windows" {
		if shell := environment.values["COMSPEC"]; shell != "" {
			return shell
		}
		return "cmd.exe"
	}
	return "/bin/sh"
}

func (environment Environment) commandValues(access protocol.ProcessAccess) map[string]string {
	if access == protocol.ProcessAccessUnrestricted {
		return maps.Clone(environment.unrestricted)
	}
	return maps.Clone(environment.scoped)
}

func environmentValues(values map[string]string) []string {
	keys := slices.Sorted(maps.Keys(values))
	result := make([]string, 0, len(keys))
	for _, key := range keys {
		result = append(result, key+"="+values[key])
	}
	return result
}

func secretVariable(key string) bool {
	key = strings.ToUpper(key)
	if key == "KEY" || strings.HasSuffix(key, "_KEY") || strings.HasSuffix(key, "_DSN") || strings.HasPrefix(key, "AWS_") {
		return true
	}
	for _, part := range []string{"TOKEN", "SECRET", "PASSW", "APIKEY", "CREDENTIAL"} {
		if strings.Contains(key, part) {
			return true
		}
	}
	return false
}

func scopedVariable(key string) bool {
	name := key
	if runtime.GOOS == "windows" {
		name = strings.ToUpper(name)
	}
	return name == "SSH_AUTH_SOCK" || name == "USER" || strings.HasPrefix(name, "LC_") || slices.Contains([]string{"http_proxy", "https_proxy", "all_proxy", "no_proxy", "ftp_proxy"}, strings.ToLower(key))
}

func terminalVariable(key string) bool {
	return slices.Contains([]string{"USER", "LOGNAME", "SHELL", "DBUS_SESSION_BUS_ADDRESS", "DISPLAY", "SSH_AUTH_SOCK", "WAYLAND_DISPLAY", "XAUTHORITY", "XDG_RUNTIME_DIR", "__CF_USER_TEXT_ENCODING", "LANGUAGE"}, key) || strings.HasPrefix(key, "LC_") || runtime.GOOS == "windows" && strings.EqualFold(key, "USERNAME")
}

func developerDirectory(path string) string {
	if !filepath.IsAbs(path) {
		return ""
	}
	if info, err := os.Stat(filepath.Join(path, "Contents", "Developer")); err == nil && info.IsDir() {
		path = filepath.Join(path, "Contents", "Developer")
	}
	path, err := filepath.EvalSymlinks(path)
	if err != nil {
		return ""
	}
	xcode := filepath.Base(path) == "Developer" && filepath.Base(filepath.Dir(path)) == "Contents" && filepath.Ext(filepath.Dir(filepath.Dir(path))) == ".app"
	if xcode {
		if info, err := os.Stat(filepath.Join(path, "usr", "bin", "xcodebuild")); err == nil && info.Mode().IsRegular() {
			return path
		}
	}
	if path == "/Library/Developer/CommandLineTools" {
		if info, err := os.Stat(filepath.Join(path, "usr", "bin", "clang")); err == nil && info.Mode().IsRegular() {
			return path
		}
	}
	return ""
}
