package shipped

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// WindowsCRTManifest records the app-local runtime copied from the native
// build's Visual Studio redistributable directory, including its notices.
const WindowsCRTManifest = "windows-crt.json"

// WindowsCRT describes one target's coherent compiler runtime artifact.
type WindowsCRT struct {
	Version int      `json:"version"`
	Target  string   `json:"target"`
	Toolset string   `json:"toolset"`
	Files   []string `json:"files"`
}

// ReadWindowsCRT reads an artifact's runtime file list. Its caller verifies
// the manifest and files against artifact checksums or the package seal.
func ReadWindowsCRT(directory string) (*WindowsCRT, error) {
	body, err := os.ReadFile(filepath.Join(directory, WindowsCRTManifest))
	if err != nil {
		return nil, err
	}
	var crt WindowsCRT
	if err := json.Unmarshal(body, &crt); err != nil || crt.Version != 1 ||
		(crt.Target != "windows/amd64" && crt.Target != "windows/arm64") || crt.Toolset == "" {
		return nil, errors.New("Windows CRT manifest is invalid")
	}
	seen := map[string]bool{}
	notices := false
	for _, name := range crt.Files {
		key := strings.ToLower(name)
		if name == "" || name != filepath.Base(name) || strings.ContainsAny(name, `/\:`) ||
			key == WindowsCRTManifest || seen[key] {
			return nil, errors.New("Windows CRT manifest has an invalid filename")
		}
		seen[key] = true
		if strings.HasPrefix(key, "windows-crt-") && !strings.HasSuffix(key, ".dll") {
			notices = true
		}
	}
	for _, name := range []string{"msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"} {
		if !seen[strings.ToLower(name)] {
			return nil, fmt.Errorf("Windows CRT manifest has no %s", name)
		}
	}
	if !notices {
		return nil, errors.New("Windows CRT manifest has no redistribution notices")
	}
	return &crt, nil
}

// CheckWindowsCRT verifies the shared runtime before a packaged library can
// load its dependencies. Other platforms do not ship the Microsoft runtime.
func (seal *Seal) CheckWindowsCRT(directory string) error {
	if !strings.HasPrefix(seal.Target, "windows/") {
		return nil
	}
	if err := seal.Check(directory, WindowsCRTManifest); err != nil {
		return err
	}
	crt, err := ReadWindowsCRT(directory)
	if err != nil {
		return err
	}
	if crt.Target != seal.Target {
		return errors.New("Windows CRT belongs to another platform")
	}
	for _, name := range crt.Files {
		if err := seal.Check(directory, name); err != nil {
			return err
		}
	}
	return nil
}
