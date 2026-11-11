package updater

import (
	"bytes"
	"crypto/ed25519"
	"encoding/json"
	"io"
	"strconv"
	"strings"

	"github.com/Masterminds/semver/v3"
	"github.com/MoeForge/moego/base64"
)

// Authenticated v1 metadata; an asset digest is never treated as publisher
// authentication.
type envelope struct {
	Version   uint32          `json:"version"`
	Payload   json.RawMessage `json:"payload"`
	Signature string          `json:"signature"`
}

type payload struct {
	Releases []Release `json:"releases"`
}

// Release is one signed package of a version for a target.
type Release struct {
	Version       string `json:"version"`
	Target        string `json:"target"`
	MinimumSystem string `json:"minimum_system"`
	Name          string `json:"name"`
	URL           string `json:"url"`
	SHA256        string `json:"sha256"`
	Size          uint64 `json:"size"`
	Bundle        string `json:"bundle"`
	Executable    string `json:"executable"`
}

// Selection is the release a check chose with the signed catalog proving it.
type Selection struct {
	Manifest []byte
	Channel  Channel
	Release  Release
}

type candidate struct {
	selection    *Selection
	hasRelease   bool
	hasTarget    bool
	incompatible bool
}

// strict decodes one JSON value, refusing unknown fields and trailing data.
func strict(data []byte, value any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return io.ErrUnexpectedEOF
	}
	return nil
}

func (c *Config) parse(data []byte) ([]Release, error) {
	if len(data) > maxManifest || len(c.Keys) == 0 {
		return nil, failure("updates_manifest_invalid", "update metadata exceeds its limit or has no trusted keys")
	}
	var signed envelope
	if err := strict(data, &signed); err != nil || signed.Payload == nil {
		return nil, failure("updates_manifest_invalid", "invalid update metadata envelope")
	}
	if signed.Version != 1 {
		return nil, failure("updates_manifest_invalid", "unsupported update metadata version")
	}
	signature, err := base64.StdEncoding.DecodeString(signed.Signature)
	if err != nil {
		return nil, failure("updates_signature_invalid", "invalid manifest signature encoding")
	}
	if len(signature) != ed25519.SignatureSize {
		return nil, failure("updates_signature_invalid", "invalid manifest signature length")
	}
	verified := false
	for _, key := range c.Keys {
		verified = verified || ed25519.Verify(key, signed.Payload, signature)
	}
	if !verified {
		return nil, failure("updates_signature_invalid", "update metadata was not signed by a trusted publisher")
	}
	var body payload
	if err := strict(signed.Payload, &body); err != nil || body.Releases == nil {
		return nil, failure("updates_manifest_invalid", "invalid authenticated update metadata")
	}
	identities := map[[2]string]bool{}
	for _, release := range body.Releases {
		if err := c.validate(release); err != nil {
			return nil, err
		}
		identity := [2]string{release.Version, release.Target}
		if identities[identity] {
			return nil, failure("updates_manifest_invalid", "duplicate update version and target")
		}
		identities[identity] = true
	}
	return body.Releases, nil
}

func (c *Config) candidate(data []byte) (candidate, error) {
	releases, err := c.parse(data)
	if err != nil {
		return candidate{}, err
	}
	current, err := semver.StrictNewVersion(c.Version)
	if err != nil {
		return candidate{}, failure("updates_manifest_invalid", "invalid current application version")
	}
	system, err := numericVersion(c.System)
	if err != nil {
		return candidate{}, err
	}
	result := candidate{hasRelease: len(releases) > 0}
	for _, release := range releases {
		if release.Target != c.Target {
			continue
		}
		result.hasTarget = true
		version := semver.MustParse(release.Version)
		if (c.Required != "" && c.Required != release.Version) || !c.Channel.accepts(version) || version.Compare(current) <= 0 {
			continue
		}
		minimum, _ := numericVersion(release.MinimumSystem)
		if less(system, minimum) {
			result.incompatible = true
			continue
		}
		result.selection = prefer(result.selection, &Selection{Manifest: bytes.Clone(data), Channel: c.Channel, Release: release})
	}
	return result, nil
}

func (c *Config) selectRelease(data []byte) (*Selection, error) {
	found, err := c.candidate(data)
	if err != nil {
		return nil, err
	}
	return finish(found)
}

func finish(found candidate) (*Selection, error) {
	if found.selection != nil {
		return found.selection, nil
	}
	if found.incompatible {
		return nil, failure("updates_system_old", "the update requires a newer operating system")
	}
	if found.hasRelease && !found.hasTarget {
		return nil, failure("updates_platform", "the manifest has no package for this target")
	}
	return nil, nil
}

func prefer(current, next *Selection) *Selection {
	if current == nil || semver.MustParse(next.Release.Version).Compare(semver.MustParse(current.Release.Version)) > 0 {
		return next
	}
	return current
}

// revalidate proves a staged selection is still the one its signed catalog
// selects for this build.
func (c *Config) revalidate(selection *Selection) error {
	if selection.Channel != c.Channel {
		return failure("updates_manifest_invalid", "the staged update channel changed")
	}
	current, err := c.selectRelease(selection.Manifest)
	if err != nil {
		return err
	}
	if current == nil {
		return failure("updates_version_old", "the selected update is not newer than this application")
	}
	if current.Release != selection.Release {
		return failure("updates_manifest_invalid", "the staged update does not match its signed selection")
	}
	return nil
}

func (c *Config) validate(release Release) error {
	if _, err := semver.StrictNewVersion(release.Version); err != nil {
		return failure("updates_manifest_invalid", "invalid release version")
	}
	if _, err := numericVersion(release.MinimumSystem); err != nil {
		return err
	}
	if err := c.validateURL(release.URL); err != nil {
		return err
	}
	bundle, executable, err := layout(release.Target)
	if err != nil {
		return err
	}
	digest := len(release.SHA256) == 64 && strings.Trim(release.SHA256, "0123456789abcdef") == ""
	if release.Bundle != bundle || release.Executable != executable || !safeRelative(release.Name) || strings.Contains(release.Name, "/") ||
		!strings.HasSuffix(release.Name, ".zip") || release.Size == 0 || release.Size > maxArchive || !digest {
		return failure("updates_manifest_invalid", "invalid release package layout, size or digest")
	}
	return nil
}

func layout(target string) (bundle, executable string, err error) {
	switch target {
	case "aarch64-apple-darwin", "x86_64-apple-darwin":
		return "Sailry.app", "Contents/MacOS/sailry-desktop", nil
	case "aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc":
		return "Sailry", "sailry-desktop.exe", nil
	}
	return "", "", failure("updates_platform", "unsupported desktop update target")
}

// safeRelative admits only plain path components.
func safeRelative(value string) bool {
	if value == "" || strings.ContainsAny(value, "\\:") || strings.HasPrefix(value, "/") {
		return false
	}
	for _, part := range strings.Split(strings.TrimSuffix(value, "/"), "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}

func numericVersion(value string) ([4]uint64, error) {
	var parts [4]uint64
	invalid := failure("updates_manifest_invalid", "invalid operating system version")
	numbers := strings.Split(value, ".")
	if len(numbers) > len(parts) {
		return parts, invalid
	}
	for i, number := range numbers {
		if number == "" || strings.Trim(number, "0123456789") != "" {
			return parts, invalid
		}
		parsed, err := strconv.ParseUint(number, 10, 64)
		if err != nil {
			return parts, invalid
		}
		parts[i] = parsed
	}
	return parts, nil
}

func less(a, b [4]uint64) bool {
	for i := range a {
		if a[i] != b[i] {
			return a[i] < b[i]
		}
	}
	return false
}
