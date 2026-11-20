package main

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"golang.org/x/mod/semver"
)

const (
	relayLatestURL        = "https://api.github.com/repos/sailry/sailry-harness/releases/latest"
	relayReleaseBase      = "https://github.com/sailry/sailry-harness/releases/download"
	relayReleaseBinary    = "sailry-relay"
	maxRelayReleaseJSON   = 1 << 20
	maxRelayChecksum      = 16 << 10
	maxRelayArchive       = 128 << 20
	maxRelayArchiveBinary = 128 << 20
	maxExpandedRelayTar   = 256 << 20
)

type relayReleases struct {
	client      *http.Client
	latestURL   string
	releaseBase string
}

func officialRelayReleases() relayReleases {
	return relayReleases{
		client:      &http.Client{Timeout: 5 * time.Minute, CheckRedirect: relayReleaseRedirect},
		latestURL:   relayLatestURL,
		releaseBase: relayReleaseBase,
	}
}

func relayReleaseRedirect(next *http.Request, via []*http.Request) error {
	u := next.URL
	if len(via) == 0 || len(via) > 5 || !strings.HasPrefix(via[0].URL.String(), relayReleaseBase+"/") ||
		u.Scheme != "https" || u.User != nil || u.Opaque != "" || u.Fragment != "" {
		return errors.New("relay release redirect refused")
	}
	switch u.Host {
	case "release-assets.githubusercontent.com", "objects.githubusercontent.com", "github-releases.githubusercontent.com":
		next.Header.Del("Referer")
		return nil
	default:
		return errors.New("relay release redirect is not a GitHub asset host")
	}
}

func (source relayReleases) latest(ctx context.Context, current, arch, directory string) (binaryPath, version string, err error) {
	if arch != "amd64" && arch != "arm64" {
		return "", "", fmt.Errorf("relay releases do not support linux/%s", arch)
	}
	current = "v" + strings.TrimPrefix(current, "v")
	if canonical := semver.Canonical(current); canonical == "" || canonical != strings.TrimSuffix(current, semver.Build(current)) {
		return "", "", errors.New("installed relay version is not valid SemVer")
	}
	var metadata bytes.Buffer
	if err := source.fetch(ctx, source.latestURL, maxRelayReleaseJSON, &metadata); err != nil {
		return "", "", fmt.Errorf("discover latest relay release: %w", err)
	}
	var release struct {
		Tag        string `json:"tag_name"`
		Draft      bool   `json:"draft"`
		Prerelease bool   `json:"prerelease"`
		Assets     []struct {
			Name string `json:"name"`
		} `json:"assets"`
	}
	if err := json.Unmarshal(metadata.Bytes(), &release); err != nil {
		return "", "", fmt.Errorf("decode latest relay release: %w", err)
	}
	if release.Draft || release.Prerelease || release.Tag == "" || semver.Canonical(release.Tag) != release.Tag || semver.Prerelease(release.Tag) != "" {
		return "", "", errors.New("latest relay release is not a stable version tag")
	}
	version = strings.TrimPrefix(release.Tag, "v")
	if semver.Compare(release.Tag, current) <= 0 {
		return "", version, nil
	}

	archiveName := "sailry-relay-linux-" + arch + ".tar.gz"
	checksumName := "SHA256SUMS-relay-linux-" + arch
	for _, name := range []string{archiveName, checksumName} {
		count := 0
		for _, asset := range release.Assets {
			if asset.Name == name {
				count++
			}
		}
		if count != 1 {
			return "", "", fmt.Errorf("relay release %s must contain exactly one %s asset (found %d)", version, name, count)
		}
	}
	base := source.releaseBase + "/" + release.Tag + "/"
	var checksums bytes.Buffer
	if err := source.fetch(ctx, base+checksumName, maxRelayChecksum, &checksums); err != nil {
		return "", "", fmt.Errorf("download %s: %w", checksumName, err)
	}
	digest, err := relayArchiveDigest(checksums.String(), archiveName)
	if err != nil {
		return "", "", err
	}
	if err := source.stageArchive(ctx, base+archiveName, archiveName, directory, digest); err != nil {
		return "", "", fmt.Errorf("stage relay %s for linux/%s: %w", version, arch, err)
	}
	return filepath.Join(directory, relayReleaseBinary), version, nil
}

func (source relayReleases) fetch(ctx context.Context, address string, limit int64, output io.Writer) error {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if err != nil {
		return err
	}
	request.Header.Set("User-Agent", "Sailry-Relay")
	request.Header.Set("Accept-Encoding", "identity")
	if address == source.latestURL {
		request.Header.Set("Accept", "application/vnd.github+json")
	}
	response, err := source.client.Do(request)
	if err != nil {
		// CDN URLs can contain signed query parameters.
		if requestErr, ok := err.(*url.Error); ok {
			return requestErr.Err
		}
		return err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return fmt.Errorf("HTTP %d", response.StatusCode)
	}
	if response.ContentLength > limit {
		return fmt.Errorf("response exceeds its %d-byte limit", limit)
	}
	size, err := io.Copy(output, io.LimitReader(response.Body, limit+1))
	if size > limit {
		return fmt.Errorf("response exceeds its %d-byte limit", limit)
	}
	return err
}

func relayArchiveDigest(data, archiveName string) ([sha256.Size]byte, error) {
	var digest [sha256.Size]byte
	found := false
	for _, line := range strings.Split(data, "\n") {
		fields := strings.Fields(line)
		if len(fields) < 2 || strings.TrimPrefix(strings.TrimPrefix(fields[1], "*"), "./") != archiveName {
			continue
		}
		if found || len(fields) != 2 || len(fields[0]) != hex.EncodedLen(len(digest)) {
			return digest, fmt.Errorf("invalid or duplicate SHA-256 checksum for %s", archiveName)
		}
		if _, err := hex.Decode(digest[:], []byte(fields[0])); err != nil {
			return digest, fmt.Errorf("invalid SHA-256 checksum for %s", archiveName)
		}
		found = true
	}
	if !found {
		return digest, fmt.Errorf("missing SHA-256 checksum for %s", archiveName)
	}
	return digest, nil
}

func (source relayReleases) stageArchive(ctx context.Context, address, name, directory string, digest [sha256.Size]byte) error {
	info, err := os.Lstat(directory)
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return errors.New("relay staging path must be a directory, not a symlink")
	}
	staging, err := os.OpenRoot(directory)
	if err != nil {
		return err
	}
	defer staging.Close()
	opened, err := staging.Stat(".")
	if err != nil {
		return err
	}
	if !os.SameFile(info, opened) {
		return errors.New("relay staging directory changed before it could be opened")
	}

	archive, err := staging.OpenFile(name, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return err
	}
	defer func() {
		_ = archive.Close()
		_ = staging.Remove(name)
	}()
	hash := sha256.New()
	if err := source.fetch(ctx, address, maxRelayArchive, io.MultiWriter(archive, hash)); err != nil {
		return fmt.Errorf("download %s: %w", name, err)
	}
	if !bytes.Equal(hash.Sum(nil), digest[:]) {
		return errors.New("relay archive checksum does not match")
	}
	if _, err := archive.Seek(0, io.SeekStart); err != nil {
		return err
	}
	if err := extractRelayBinary(ctx, archive, staging); err != nil {
		return fmt.Errorf("extract relay binary: %w", err)
	}
	return nil
}

func extractRelayBinary(ctx context.Context, input io.Reader, staging *os.Root) (result error) {
	gzipReader, err := gzip.NewReader(input)
	if err != nil {
		return err
	}
	defer gzipReader.Close()
	expanded := &io.LimitedReader{R: gzipReader, N: maxExpandedRelayTar + 1}
	reader := tar.NewReader(expanded)
	found := false
	defer func() {
		if result != nil && found {
			_ = staging.Remove(relayReleaseBinary)
		}
	}()
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		header, err := reader.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return err
		}
		name := strings.TrimPrefix(header.Name, "./")
		if header.Typeflag == tar.TypeDir {
			if header.Name == "./" {
				continue
			}
			name = strings.TrimSuffix(name, "/")
		}
		if !fs.ValidPath(name) || strings.Contains(name, `\`) {
			return fmt.Errorf("invalid relay archive path %q", header.Name)
		}
		if name != relayReleaseBinary {
			continue
		}
		if found {
			return errors.New("relay archive contains duplicate sailry-relay entries")
		}
		if header.Typeflag != tar.TypeReg || header.Size <= 0 || header.Size > maxRelayArchiveBinary {
			return errors.New("relay archive must contain a nonempty regular sailry-relay file no larger than 128 MiB")
		}
		output, err := staging.OpenFile(relayReleaseBinary, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o700)
		if err != nil {
			return err
		}
		found = true
		_, copyErr := io.Copy(output, reader)
		if copyErr == nil {
			copyErr = output.Sync()
		}
		if err := errors.Join(copyErr, output.Close()); err != nil {
			return err
		}
	}
	if !found {
		return errors.New("relay archive has no top-level sailry-relay file")
	}
	// A tar can end before the gzip trailer. Verify it and allow only tar padding.
	var padding [32 << 10]byte
	for {
		n, err := expanded.Read(padding[:])
		if expanded.N == 0 {
			return errors.New("expanded relay archive exceeds its size limit")
		}
		for _, b := range padding[:n] {
			if b != 0 {
				return errors.New("unexpected data after the relay tar archive")
			}
		}
		if err == io.EOF {
			return ctx.Err()
		}
		if err != nil {
			return err
		}
	}
}
