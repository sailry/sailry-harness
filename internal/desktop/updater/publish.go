package updater

import (
	"archive/zip"
	"crypto"
	"crypto/ed25519"
	"crypto/sha256"
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"os"
	"path/filepath"
	"slices"

	"github.com/Masterminds/semver/v3"
	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/identity"
)

// The publisher's side of the signed update-v1 format, run by the release
// workflow through cmd/update-feed. It writes exactly what the checks in
// this package accept and proves so with them before it finishes.

// Publish signs the update archives of one release and writes into
// directory the files that release carries for the updater: each
// Sailry-<target>.zip, its .update.json and the signed platform catalogs.
// The source ZIPs contain sealed macOS bundles or the portable Windows app.
// seed is the trusted publisher key's 32-byte seed.
func Publish(seed []byte, version, directory string, unsigned map[string]string) error {
	return PublishRelease(identity.OfficialDesktop().Release, seed, version, directory, unsigned)
}

func PublishRelease(product identity.DesktopRelease, seed []byte, version, directory string, unsigned map[string]string) error {
	config, err := releaseInputs(product)
	if err != nil {
		return err
	}
	if len(seed) != ed25519.SeedSize {
		return errors.New("the update signing key must have 32 bytes")
	}
	return publish(ed25519.NewKeyFromSeed(seed), &config, version, directory, unsigned)
}

func publish(key ed25519.PrivateKey, config *Config, version, directory string, unsigned map[string]string) error {
	if !slices.ContainsFunc(config.Keys, func(trusted ed25519.PublicKey) bool { return trusted.Equal(key.Public()) }) {
		return errors.New("the update signing key is outside the trusted public keys")
	}
	parsed, err := semver.StrictNewVersion(version)
	if err != nil || parsed.Metadata() != "" {
		return errors.New("invalid release version")
	}
	// Discovery admits a macOS release only with both targets.
	targets := slices.Sorted(maps.Keys(unsigned))
	expected := []string{"aarch64-apple-darwin", "x86_64-apple-darwin"}
	if _, ok := unsigned["x86_64-pc-windows-msvc"]; ok {
		expected = append(expected, "x86_64-pc-windows-msvc")
	}
	if !slices.Equal(targets, expected) {
		return errors.New("expected both macOS archives and optionally the Windows x64 archive")
	}
	release := published{version: parsed, downloads: config.Downloads + "/v" + version}
	catalogs := map[string][]Release{}
	for _, target := range targets {
		bundle, executable, err := config.layout(target)
		if err != nil {
			return err
		}
		// The Go runtime requires Windows 10 / Windows Server 2016 or later.
		minimum := "10.0"
		if target != "x86_64-pc-windows-msvc" {
			minimum, err = minimumSystem(unsigned[target], bundle)
			if err != nil {
				return err
			}
		}
		name := config.packageIdentity().Name + "-" + target + ".zip"
		path := filepath.Join(directory, name)
		if err := signedCopy(key, unsigned[target], path); err != nil {
			return err
		}
		digest, size, err := measure(path)
		if err != nil {
			return err
		}
		entry := Release{Version: version, Target: target, MinimumSystem: minimum, Name: name, URL: release.downloads + "/" + name,
			SHA256: digest, Size: size, Bundle: bundle, Executable: executable}
		metadata, err := json.Marshal(entry)
		if err != nil {
			return err
		}
		if err := writeNew(path+".update.json", append(metadata, '\n')); err != nil {
			return err
		}
		if err := verifyArchive(path, entry, config.Keys); err != nil {
			return err
		}
		work, err := os.MkdirTemp("", "sailry-update-")
		if err != nil {
			return err
		}
		_, err = config.extract(path, entry, work)
		if removal := os.RemoveAll(work); err == nil {
			err = removal
		}
		if err != nil {
			return fmt.Errorf("%s: %w", entry.Name, err)
		}
		file := catalogName(target)
		catalogs[file] = append(catalogs[file], entry)
	}
	for name, releases := range catalogs {
		body, err := json.Marshal(payload{Releases: releases})
		if err != nil {
			return err
		}
		catalog, err := json.Marshal(envelope{Version: 1, Payload: body, Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(key, body))})
		if err != nil {
			return err
		}
		if err := release.authenticate(catalog, config); err != nil {
			return err
		}
		if err := writeNew(filepath.Join(directory, name), append(catalog, '\n')); err != nil {
			return err
		}
	}
	return nil
}

// signedCopy copies a ZIP behind the ed25519ph signature block the updater
// reads. Entry offsets count from the start of the file, so the result stays
// an ordinary ZIP for every reader.
func signedCopy(key ed25519.PrivateKey, source, destination string) error {
	reader, err := zip.OpenReader(source)
	if err != nil {
		return err
	}
	defer reader.Close()
	output, err := os.OpenFile(destination, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return err
	}
	defer output.Close()
	block := append(append([]byte{}, signatureMagic...), 1, 0)
	header := int64(len(block) + ed25519.SignatureSize)
	if _, err := output.Write(make([]byte, header)); err != nil {
		return err
	}
	writer := zip.NewWriter(output)
	writer.SetOffset(header)
	for _, entry := range reader.File {
		if err := writer.Copy(entry); err != nil {
			return err
		}
	}
	if err := writer.SetComment(reader.Comment); err != nil {
		return err
	}
	if err := writer.Close(); err != nil {
		return err
	}
	if _, err := output.Seek(header, io.SeekStart); err != nil {
		return err
	}
	prehash := sha512.New()
	if _, err := io.Copy(prehash, output); err != nil {
		return err
	}
	signature, err := key.Sign(nil, prehash.Sum(nil), &ed25519.Options{Hash: crypto.SHA512, Context: filepath.Base(destination)})
	if err != nil {
		return err
	}
	if _, err := output.WriteAt(append(block, signature...), 0); err != nil {
		return err
	}
	return output.Close()
}

// minimumSystem reads the system version the packaged application declares,
// which the updater compares with the signed metadata.
func minimumSystem(archive, bundle string) (string, error) {
	reader, err := zip.OpenReader(archive)
	if err != nil {
		return "", err
	}
	defer reader.Close()
	for _, entry := range reader.File {
		if entry.Name != bundle+"/Contents/Info.plist" {
			continue
		}
		source, err := entry.Open()
		if err != nil {
			return "", err
		}
		defer source.Close()
		data, err := io.ReadAll(io.LimitReader(source, 128*1024))
		if err != nil {
			return "", err
		}
		values, err := plistStrings(data)
		if err != nil || values["LSMinimumSystemVersion"] == "" {
			return "", errors.New("the application plist declares no minimum system version")
		}
		return values["LSMinimumSystemVersion"], nil
	}
	return "", fmt.Errorf("the archive holds no %s", bundle)
}

func measure(path string) (string, uint64, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", 0, err
	}
	defer file.Close()
	digest := sha256.New()
	size, err := io.Copy(digest, file)
	return hex.EncodeToString(digest.Sum(nil)), uint64(size), err
}

// writeNew never replaces an existing file.
func writeNew(path string, data []byte) error {
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return err
	}
	if _, err := file.Write(data); err != nil {
		file.Close()
		return err
	}
	return file.Close()
}
