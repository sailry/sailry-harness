// Package computer locates the bundled computer-use helper, reads the
// execution device's helper grants and owns the helper's cursor-overlay
// sessions. The helper is never searched on PATH and never downloaded.
package computer

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Token is the stdio MCP command a package declares to launch the helper.
const Token = "${SAILRY_COMPUTER_USE}"

// session lists the graphical-session inputs the Linux helper needs to reach
// AT-SPI and the display.
var session = []string{
	"DBUS_SESSION_BUS_ADDRESS",
	"DISPLAY",
	"WAYLAND_DISPLAY",
	"XAUTHORITY",
	"XDG_CURRENT_DESKTOP",
	"XDG_RUNTIME_DIR",
	"XDG_SESSION_TYPE",
}

// Executable returns the execution Node's packaged helper.
func Executable() (string, error) {
	if runtime.GOOS == "darwin" && macOSMajor() < 13 {
		return "", unavailable("computer use requires macOS 13 or later")
	}
	executable, err := os.Executable()
	if err != nil {
		return "", unavailable("execution Node location is unavailable")
	}
	path, err := locate(runtime.GOOS, executable, os.Getenv("SAILRY_HELPERS_DIR"))
	if err != nil {
		return "", err
	}
	if err := validate(path); err != nil {
		return "", err
	}
	return path, nil
}

// locate names the helper beside the Node executable: a macOS bundle keeps it
// in Contents/Resources/helpers, where packaging puts shipped files, every
// other layout in a sibling helpers directory.
func locate(goos, executable, helpers string) (string, error) {
	var name string
	switch goos {
	case "darwin":
		name = filepath.Join("Sailry Computer Use.app", "Contents", "MacOS", "agent-desktop")
	case "windows":
		name = "agent-desktop.exe"
	case "linux":
		name = "agent-desktop"
	default:
		return "", unavailable("computer use is unavailable on this platform")
	}
	parent := filepath.Dir(executable)
	if parent == executable || parent == "." {
		return "", unavailable("execution Node location is unavailable")
	}
	if helpers == "" {
		contents := filepath.Dir(parent)
		if goos == "darwin" && filepath.Base(parent) == "MacOS" && filepath.Base(contents) == "Contents" {
			helpers = filepath.Join(contents, "Resources", "helpers")
		} else {
			helpers = filepath.Join(parent, "helpers")
		}
	}
	return filepath.Join(helpers, name), nil
}

func validate(path string) error {
	metadata, err := os.Stat(path)
	if err != nil {
		return unavailable("computer helper is missing")
	}
	if !metadata.Mode().IsRegular() {
		return unavailable("computer helper is not a regular executable")
	}
	if runtime.GOOS != "windows" && metadata.Mode().Perm()&0o111 == 0 {
		return unavailable("computer helper is not executable")
	}
	return nil
}

// Session passes the Node's graphical session to the Linux helper; declared
// values take precedence.
func Session(env map[string]string) {
	if runtime.GOOS == "linux" {
		inherit(env, os.LookupEnv)
	}
}

func inherit(env map[string]string, read func(string) (string, bool)) {
	for _, name := range session {
		if _, declared := env[name]; declared {
			continue
		}
		if value, found := read(name); found {
			env[name] = value
		}
	}
}

// PrepareHome makes the helper home a private directory inside the package
// data directory.
func PrepareHome(data, home string) error {
	if runtime.GOOS != "windows" {
		if err := os.Chmod(data, 0o700); err != nil {
			return err
		}
	}
	if err := os.MkdirAll(home, 0o700); err != nil {
		return err
	}
	metadata, err := os.Lstat(home)
	if err != nil {
		return err
	}
	if !metadata.IsDir() {
		return errors.New("computer helper home must be a directory")
	}
	if runtime.GOOS != "windows" {
		return os.Chmod(home, 0o700)
	}
	return nil
}

func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}
