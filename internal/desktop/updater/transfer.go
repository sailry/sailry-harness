package updater

import (
	"context"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"time"
)

// Network work stages a verified file separately from installation.

// Progress is a download's position, or its verification.
type Progress struct {
	Copied, Total uint64
	Verifying     bool
}

// staged is a downloaded, verified and extracted release. Its directory is
// removed when the update is abandoned.
type staged struct {
	directory string
	archive   string
	selection *Selection
}

func cancelled() *Failure { return failure("updates_cancelled", "update transfer was cancelled") }

// newClient follows only safe redirects and never retries.
func newClient(config *Config) *http.Client {
	return &http.Client{CheckRedirect: func(next *http.Request, via []*http.Request) error {
		next.Header.Del("Referer")
		if err := config.validateURL(next.URL.String()); err != nil {
			return err
		}
		if via[len(via)-1].URL.Scheme == "https" && next.URL.Scheme != "https" {
			return failure("updates_network_failed", "update redirects must not downgrade HTTPS")
		}
		// The first previous request is the initial one, not a redirect hop.
		if len(via) > 5 {
			return failure("updates_network_failed", "update redirects exceed their limit or repeat a requested URL")
		}
		for _, previous := range via {
			if previous.URL.String() == next.URL.String() {
				return failure("updates_network_failed", "update redirects exceed their limit or repeat a requested URL")
			}
		}
		return nil
	}}
}

func get(ctx context.Context, client *http.Client, config *Config, source string) (*http.Response, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, source, nil)
	if err != nil {
		return nil, err
	}
	request.Header.Set("User-Agent", "Sailry/"+config.Version)
	return client.Do(request)
}

// transferFailure names a failed request: the caller's cancellation, a
// refused redirect, or the network.
func transferFailure(ctx context.Context, err error, detail string) *Failure {
	if errors.Is(ctx.Err(), context.Canceled) {
		return cancelled()
	}
	var refused *Failure
	if errors.As(err, &refused) {
		return refused
	}
	return failure("updates_network_failed", detail)
}

// check asks the source for the newest signed release this build accepts.
// It returns nil when the build is current.
func check(ctx context.Context, client *http.Client, config *Config) (*Selection, error) {
	releases, err := list(ctx, client, config)
	if err != nil {
		return nil, err
	}
	var combined candidate
	for _, release := range releases {
		data, err := body(ctx, client, config, release.catalog, maxManifest)
		if err != nil {
			return nil, err
		}
		if err := release.authenticate(data, config); err != nil {
			return nil, err
		}
		found, err := config.candidate(data)
		if err != nil {
			return nil, err
		}
		if found.selection != nil {
			combined.selection = prefer(combined.selection, found.selection)
		}
		combined.hasRelease = combined.hasRelease || found.hasRelease
		combined.hasTarget = combined.hasTarget || found.hasTarget
		combined.incompatible = combined.incompatible || found.incompatible
		if combined.selection != nil {
			break
		}
	}
	if config.Required != "" && combined.selection == nil && !combined.incompatible {
		return nil, failure("compatibility_update_unavailable", "the requested Desktop version has no compatible signed release")
	}
	if errors.Is(ctx.Err(), context.Canceled) {
		return nil, cancelled()
	}
	return finish(combined)
}

func body(ctx context.Context, client *http.Client, config *Config, source string, limit int64) ([]byte, error) {
	if err := config.validateURL(source); err != nil {
		return nil, err
	}
	request, stop := context.WithTimeout(ctx, 45*time.Second)
	defer stop()
	response, err := get(request, client, config, source)
	if err != nil {
		return nil, transferFailure(ctx, err, "the update metadata request failed")
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, failure("updates_source_unready", "the update metadata server returned HTTP "+response.Status[:3])
	}
	if response.ContentLength > limit {
		return nil, invalid("update metadata exceeds its size limit")
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil {
		return nil, transferFailure(ctx, err, "the update metadata transfer was interrupted")
	}
	if int64(len(data)) > limit {
		return nil, invalid("update metadata exceeds its size limit")
	}
	return data, nil
}

// download stages the selected archive under root, verifies it against the
// signed catalog and the publisher's keys, and extracts it for validation.
func download(ctx context.Context, client *http.Client, config *Config, selection *Selection, root string, progress func(Progress)) (result *staged, err error) {
	if err := config.revalidate(selection); err != nil {
		return nil, err
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return nil, ioFailure(err)
	}
	directory, err := os.MkdirTemp(root, "download-")
	if err != nil {
		return nil, ioFailure(err)
	}
	defer func() {
		if err != nil {
			_ = os.RemoveAll(directory)
		}
	}()
	release := selection.Release
	archive := filepath.Join(directory, release.Name)
	destination, err := os.OpenFile(archive, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return nil, ioFailure(err)
	}
	defer destination.Close()
	progress(Progress{Total: release.Size})
	request, stop := context.WithTimeout(ctx, 30*time.Minute)
	defer stop()
	source, err := http.NewRequestWithContext(request, http.MethodGet, release.URL, nil)
	if err != nil {
		return nil, invalid("invalid update URL")
	}
	source.Header.Set("User-Agent", "Sailry/"+config.Version)
	source.Header.Set("Accept-Encoding", "identity")
	response, err := client.Do(source)
	if err != nil {
		return nil, transferFailure(ctx, err, "the update asset request failed")
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, failure("updates_network_failed", "the update asset server rejected the request")
	}
	if response.ContentLength >= 0 && uint64(response.ContentLength) != release.Size {
		return nil, failure("updates_package_invalid", "the update asset length differs from signed metadata")
	}
	var copied uint64
	last := time.Now()
	buffer := make([]byte, 64*1024)
	for {
		n, readErr := response.Body.Read(buffer)
		if n > 0 {
			copied += uint64(n)
			if copied > release.Size {
				return nil, failure("updates_package_invalid", "the update asset exceeds its signed size")
			}
			if _, err := destination.Write(buffer[:n]); err != nil {
				return nil, ioFailure(err)
			}
			if copied == release.Size || time.Since(last) >= 75*time.Millisecond {
				progress(Progress{Copied: copied, Total: release.Size})
				last = time.Now()
			}
		}
		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			return nil, transferFailure(ctx, readErr, "the update asset transfer was interrupted")
		}
	}
	if copied != release.Size {
		return nil, failure("updates_package_invalid", "the update asset is incomplete")
	}
	if err := destination.Sync(); err != nil {
		return nil, ioFailure(err)
	}
	if err := destination.Close(); err != nil {
		return nil, ioFailure(err)
	}
	if ctx.Err() != nil {
		return nil, cancelled()
	}
	progress(Progress{Copied: copied, Total: release.Size, Verifying: true})
	// Cancellation waits for this read-only tail to end.
	if err := verifyArchive(archive, release, config.Keys); err != nil {
		return nil, err
	}
	contents := filepath.Join(directory, "contents")
	if err := os.Mkdir(contents, 0o700); err != nil {
		return nil, ioFailure(err)
	}
	if _, err := extract(archive, release, contents); err != nil {
		return nil, err
	}
	if ctx.Err() != nil {
		return nil, cancelled()
	}
	return &staged{directory: directory, archive: archive, selection: selection}, nil
}
