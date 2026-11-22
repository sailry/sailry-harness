package wsl

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"golang.org/x/mod/semver"
)

// ResolveRelease selects an official Host tag before invoking the installer.
func ResolveRelease(ctx context.Context, release identity.HostRelease) (string, error) {
	client := &http.Client{
		Timeout:       30 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	return resolveRelease(ctx, release, client)
}

func resolveRelease(ctx context.Context, release identity.HostRelease, client *http.Client) (string, error) {
	if !release.Published() {
		return link.ProductVersion, nil
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, identity.OfficialReleases+"/latest", nil)
	if err != nil {
		return "", err
	}
	request.Header.Set("Accept", "application/vnd.github+json")
	response, err := client.Do(request)
	if err != nil {
		return "", unavailable("official Host release discovery failed")
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return "", unavailable("official Host release discovery failed")
	}
	const maxRelease = 1024 * 1024
	body, err := io.ReadAll(io.LimitReader(response.Body, maxRelease+1))
	if err != nil || len(body) > maxRelease {
		return "", unavailable("official Host release metadata could not be read")
	}
	var latest struct {
		Tag        string `json:"tag_name"`
		Draft      bool   `json:"draft"`
		Prerelease bool   `json:"prerelease"`
	}
	if err := json.Unmarshal(body, &latest); err != nil {
		return "", unavailable("official Host release metadata is invalid")
	}
	version, tagged := strings.CutPrefix(latest.Tag, "v")
	if !tagged || latest.Draft || latest.Prerelease || !validReleaseVersion(version) || semver.Prerelease(latest.Tag) != "" {
		return "", unavailable("official Host release has no stable version")
	}
	return version, nil
}

func validReleaseVersion(version string) bool {
	tag := "v" + version
	return semver.IsValid(tag) && semver.Build(tag) == "" && semver.Canonical(tag) == tag
}
