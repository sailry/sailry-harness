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

// ResolveRelease selects a published Host tag before invoking the installer.
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
	source := release.Releases() + "/latest"
	code := release == identity.CodeHost()
	if code {
		source = release.Releases() + "?per_page=100"
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, source, nil)
	if err != nil {
		return "", err
	}
	request.Header.Set("Accept", "application/vnd.github+json")
	response, err := client.Do(request)
	if err != nil {
		return "", unavailable("Host release discovery failed")
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return "", unavailable("Host release discovery failed")
	}
	const maxRelease = 1024 * 1024
	body, err := io.ReadAll(io.LimitReader(response.Body, maxRelease+1))
	if err != nil || len(body) > maxRelease {
		return "", unavailable("Host release metadata could not be read")
	}
	type published struct {
		Tag        string `json:"tag_name"`
		Draft      bool   `json:"draft"`
		Prerelease bool   `json:"prerelease"`
	}
	var releases []published
	if code {
		if err := json.Unmarshal(body, &releases); err != nil {
			return "", unavailable("Host release metadata is invalid")
		}
	} else {
		var latest published
		if err := json.Unmarshal(body, &latest); err != nil {
			return "", unavailable("Host release metadata is invalid")
		}
		releases = append(releases, latest)
	}
	var selected string
	for _, candidate := range releases {
		version, tagged := strings.CutPrefix(candidate.Tag, "v")
		if !tagged || candidate.Draft || candidate.Prerelease || !validReleaseVersion(version) || semver.Prerelease(candidate.Tag) != "" {
			continue
		}
		if selected == "" || semver.Compare(candidate.Tag, "v"+selected) > 0 {
			selected = version
		}
	}
	if selected == "" {
		return "", unavailable("Host release has no stable version")
	}
	return selected, nil
}

func validReleaseVersion(version string) bool {
	tag := "v" + version
	return semver.IsValid(tag) && semver.Build(tag) == "" && semver.Canonical(tag) == tag
}
