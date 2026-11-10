// Package updater checks, downloads, verifies and installs the Desktop's
// signed update-v1 releases. Controller updates never replace or shut down a
// remote execution Host.
package updater

import (
	"crypto/ed25519"
	_ "embed"
	"encoding/base64"
	"net/url"
	"runtime"
	"strings"

	"filippo.io/edwards25519"
	"github.com/Masterminds/semver/v3"
)

const (
	// Releases and Downloads are the official release source.
	Releases    = "https://api.github.com/repos/sailry/sailry-harness/releases"
	Downloads   = "https://github.com/sailry/sailry-harness/releases/download"
	maxManifest = 1024 * 1024
	maxArchive  = 2 * 1024 * 1024 * 1024
)

// Release-owned trust configuration. Runtime environment variables cannot
// replace its keys.
//
//go:embed update-public-keys.txt
var publicKeys string

// Failure is an update failure: its i18n key and the detail for the dialog.
type Failure struct {
	Key    string
	Detail string
}

func (f *Failure) Error() string { return f.Detail }

func failure(key, detail string) *Failure { return &Failure{Key: key, Detail: detail} }

func ioFailure(err error) *Failure { return failure("updates_io_failed", err.Error()) }

// asFailure keeps a Failure and files anything else under I/O.
func asFailure(err error) *Failure {
	if f, ok := err.(*Failure); ok {
		return f
	}
	return ioFailure(err)
}

// Channel is the release channel.
type Channel int

const (
	Stable Channel = iota
	Experience
)

// InitialChannel is the channel of a first launch: a prerelease build follows
// Experience.
func InitialChannel(version string) Channel {
	if v, err := semver.StrictNewVersion(version); err == nil && v.Prerelease() != "" {
		return Experience
	}
	return Stable
}

func (c Channel) accepts(version *semver.Version) bool {
	return c == Experience || version.Prerelease() == ""
}

// Config is one check's source, trust and identity of the running build.
type Config struct {
	Source    string
	Downloads string
	Required  string // an exact version a newer Host asks for, or empty
	Channel   Channel
	Keys      []ed25519.PublicKey
	Target    string
	Version   string
	System    string
	local     bool // tests admit plain HTTP on the loopback interface
}

// Target is the release target of this build, empty where updates are
// unsupported.
func Target() string {
	arch := map[string]string{"arm64": "aarch64", "amd64": "x86_64"}[runtime.GOARCH]
	system := map[string]string{"darwin": "apple-darwin", "windows": "pc-windows-msvc"}[runtime.GOOS]
	if arch == "" || system == "" {
		return ""
	}
	return arch + "-" + system
}

// ReleaseConfig is the official configuration for the running version.
func ReleaseConfig(channel Channel, version string) (Config, error) {
	keys, err := parseKeys(publicKeys)
	if err != nil {
		return Config{}, err
	}
	system, err := systemVersion()
	if err != nil {
		return Config{}, err
	}
	return Config{Source: Releases, Downloads: Downloads, Channel: channel, Keys: keys, Target: Target(), Version: version, System: system}, nil
}

func parseKeys(encoded string) ([]ed25519.PublicKey, error) {
	var keys []ed25519.PublicKey
	for _, value := range strings.FieldsFunc(encoded, func(r rune) bool { return r == ',' || r == ' ' || r == '\t' || r == '\n' || r == '\r' }) {
		bytes, err := base64.StdEncoding.DecodeString(value)
		if err != nil {
			return nil, failure("updates_source_unready", "invalid configured update public key")
		}
		if len(bytes) != ed25519.PublicKeySize {
			return nil, failure("updates_source_unready", "update public keys must have 32 bytes")
		}
		point, err := new(edwards25519.Point).SetBytes(bytes)
		if err != nil {
			return nil, failure("updates_source_unready", "invalid configured update public key")
		}
		if new(edwards25519.Point).MultByCofactor(point).Equal(edwards25519.NewIdentityPoint()) == 1 {
			return nil, failure("updates_source_unready", "weak update public key")
		}
		keys = append(keys, ed25519.PublicKey(bytes))
	}
	if len(keys) == 0 {
		return nil, failure("updates_source_unready", "no trusted update public key is configured")
	}
	return keys, nil
}

func (c *Config) validateURL(value string) error {
	u, err := url.Parse(value)
	if err != nil {
		return failure("updates_manifest_invalid", "invalid update URL")
	}
	host := u.Hostname()
	fixture := c.local && u.Scheme == "http" && (host == "127.0.0.1" || host == "::1" || host == "localhost")
	if (u.Scheme != "https" && !fixture) || host == "" || u.User != nil || u.Fragment != "" || strings.Contains(value, "#") {
		return failure("updates_manifest_invalid", "update URLs require HTTPS without credentials or fragments")
	}
	return nil
}
