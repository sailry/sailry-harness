// Package identity defines build-owned application identity and release inputs.
package identity

import (
	_ "embed"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const (
	OfficialVersion        = "0.2.11"
	SourceProfile          = "~/.sailry-go"
	InstalledProfile       = "~/.sailry"
	OfficialRepository     = "sailry/sailry-harness"
	OfficialReleases       = "https://api.github.com/repos/" + OfficialRepository + "/releases"
	OfficialDownloads      = "https://github.com/" + OfficialRepository + "/releases/download"
	OfficialWebsite        = "https://sailry.dev"
	OfficialFeedback       = "https://github.com/" + OfficialRepository + "/issues"
	OfficialHostProfile    = "." + officialHostName + "-host"
	OfficialHostVersionTag = "Sailry Host "
)

//go:embed update-public-keys.txt
var officialUpdateKeys string

type DesktopPackage struct {
	Name       string // Bundle directory and release archive prefix.
	ID         string // Native bundle identifier.
	Executable string // Executable basename, without a Windows extension.
}

type DesktopRelease struct {
	Package    DesktopPackage
	Source     string
	Downloads  string
	PublicKeys string // Base64 Ed25519 public keys selected at build time.
}

type Desktop struct {
	Release        DesktopRelease
	Version        string
	DefaultProfile string
	DefaultEntry   string
	HostRelease    HostRelease
	Website        string
	Feedback       string
}

func OfficialDesktop() Desktop {
	return Desktop{
		Release: DesktopRelease{
			Package: DesktopPackage{Name: "Sailry", ID: "ai.sailry.desktop", Executable: "sailry-desktop"},
			Source:  OfficialReleases, Downloads: OfficialDownloads, PublicKeys: officialUpdateKeys,
		},
		Version: OfficialVersion, DefaultProfile: SourceProfile, DefaultEntry: "page:conversation",
		Website: OfficialWebsite, Feedback: OfficialFeedback,
	}
}

// DataDir resolves the default profile or an explicit command-line override.
func (product Desktop) DataDir(override string) (string, error) {
	if override != "" {
		return filepath.Abs(override)
	}
	return ResolveProfile(product.DefaultProfile)
}

func ResolveProfile(location string) (string, error) {
	if filepath.IsAbs(location) {
		return location, nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("user home directory is unavailable; specify --data-dir: %w", err)
	}
	return profileFromHome(location, home)
}

func profileFromHome(location, home string) (string, error) {
	if !filepath.IsAbs(home) {
		return "", errors.New("user home directory is unavailable; specify --data-dir")
	}
	if !strings.HasPrefix(location, "~/") {
		return "", errors.New("default data directory must be absolute or home-relative; specify --data-dir")
	}
	return filepath.Join(home, filepath.FromSlash(strings.TrimPrefix(location, "~/"))), nil
}
