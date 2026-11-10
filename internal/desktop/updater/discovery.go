package updater

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"

	"github.com/Masterminds/semver/v3"
)

// GitHub discovery is untrusted; only the signed catalog admits an update.
const (
	pageSize  = 100
	pageLimit = 10
	maxPage   = 8 * 1024 * 1024
)

type listedRelease struct {
	TagName    string        `json:"tag_name"`
	Draft      bool          `json:"draft"`
	Prerelease bool          `json:"prerelease"`
	Assets     []listedAsset `json:"assets"`
}

type listedAsset struct {
	Name               string `json:"name"`
	State              string `json:"state"`
	Size               uint64 `json:"size"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

type published struct {
	version   *semver.Version
	catalog   string
	downloads string
}

func invalid(message string) *Failure { return failure("updates_manifest_invalid", message) }

// authenticate requires the signed catalog to describe exactly the version
// and assets of the release that published it.
func (p published) authenticate(data []byte, config *Config) error {
	releases, err := config.parse(data)
	if err != nil {
		return err
	}
	if len(releases) == 0 {
		return invalid("the signed catalog does not match its exact published version")
	}
	for _, release := range releases {
		if release.Version != p.version.String() || release.Name != "Sailry-"+release.Target+".zip" || release.URL != p.downloads+"/"+release.Name {
			return invalid("the signed catalog does not match its exact published version")
		}
	}
	return nil
}

// list returns the complete published releases newer than this build, newest
// first.
func list(ctx context.Context, client *http.Client, config *Config) ([]published, error) {
	var found []published
	for page := 1; page <= pageLimit; page++ {
		source, err := url.Parse(config.Source)
		if err != nil {
			return nil, invalid("invalid release discovery URL")
		}
		query := source.Query()
		query.Set("per_page", strconv.Itoa(pageSize))
		query.Set("page", strconv.Itoa(page))
		source.RawQuery = query.Encode()
		data, err := body(ctx, client, config, source.String(), maxPage)
		if err != nil {
			return nil, err
		}
		var releases []listedRelease
		if err := json.Unmarshal(data, &releases); err != nil {
			return nil, invalid("invalid GitHub release discovery response")
		}
		for _, release := range releases {
			value, err := release.published(config)
			if err != nil {
				return nil, err
			}
			if value != nil {
				found = append(found, *value)
			}
		}
		if len(releases) < pageSize {
			slices.SortStableFunc(found, func(a, b published) int { return b.version.Compare(a.version) })
			return found, nil
		}
	}
	return nil, invalid("release discovery exceeds its page limit")
}

func (r listedRelease) published(config *Config) (*published, error) {
	tag, prefixed := strings.CutPrefix(r.TagName, "v")
	version, err := semver.StrictNewVersion(tag)
	if !prefixed || err != nil {
		return nil, nil
	}
	current, err := semver.StrictNewVersion(config.Version)
	if err != nil {
		return nil, invalid("invalid current application version")
	}
	if r.Draft || version.Metadata() != "" || (r.Prerelease && version.Prerelease() == "") || !config.Channel.accepts(version) ||
		version.Compare(current) <= 0 || (config.Required != "" && config.Required != version.String()) {
		return nil, nil
	}
	downloads := config.Downloads + "/" + r.TagName
	targets := []string{"aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"}
	if strings.HasSuffix(config.Target, "apple-darwin") {
		targets = []string{"aarch64-apple-darwin", "x86_64-apple-darwin"}
	}
	names := []string{"update-v1.json"}
	for _, target := range targets {
		archive := "Sailry-" + target
		names = append(names, archive+".zip", archive+".zip.update.json", "SHA256SUMS-desktop-"+target)
		if strings.HasSuffix(target, "apple-darwin") {
			names = append(names, archive+".dmg")
		}
	}
	for _, name := range names {
		var asset *listedAsset
		count := 0
		for i := range r.Assets {
			if r.Assets[i].Name == name {
				if asset == nil {
					asset = &r.Assets[i]
				}
				count++
			}
		}
		if asset == nil {
			return nil, nil
		}
		if count > 1 || asset.BrowserDownloadURL != downloads+"/"+name {
			return nil, invalid("the published signed release asset set is invalid")
		}
		if asset.State != "uploaded" || asset.Size == 0 {
			return nil, nil
		}
		if name == "update-v1.json" && asset.Size > maxManifest {
			return nil, invalid("the published signed catalog exceeds its size limit")
		}
	}
	return &published{version: version, catalog: downloads + "/update-v1.json", downloads: downloads}, nil
}
