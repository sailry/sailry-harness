package theme

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/Masterminds/semver/v3"
	"github.com/egoist/mygo/netproxy"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const ThemeCatalogURL = "https://sailry.dev/themes/catalog.json"
const ThemeRawURL = "https://raw.githubusercontent.com/sailry/sailry-themes/"

type CatalogResource struct {
	Path   string `json:"path"`
	Bytes  int    `json:"bytes"`
	Digest string `json:"digest"`
}

type CatalogEntry struct {
	ID                 string            `json:"id"`
	Path               string            `json:"path"`
	Name               string            `json:"name"`
	Version            string            `json:"package_version"`
	Author             string            `json:"author"`
	License            string            `json:"license"`
	Description        *string           `json:"description"`
	DescriptionLocales map[string]string `json:"description_locales"`
	Homepage           *string           `json:"homepage"`
	Digest             string            `json:"digest"`
	Resources          []CatalogResource `json:"resources"`
	Modes              []string          `json:"modes"`
	Previews           map[string]string `json:"previews"`
}

type CatalogListing struct {
	Version     uint32         `json:"version"`
	Commit      string         `json:"source_commit,omitempty"`
	Themes      []CatalogEntry `json:"themes"`
	Unavailable bool           `json:"-"`
}

type ThemeSelection struct {
	Commit string
	Entry  CatalogEntry
}

type Market struct {
	Profile    *Profile
	Client     *http.Client
	CatalogURL string
	RawURL     string
	mu         sync.Mutex
	cached     *CatalogListing
	until      time.Time
}

type themeSourceUnavailable struct{ cause error }

func (err *themeSourceUnavailable) Error() string { return err.cause.Error() }
func (err *themeSourceUnavailable) Unwrap() error { return err.cause }

type savedCatalog struct {
	Version uint32         `json:"version"`
	Commit  string         `json:"commit"`
	Catalog CatalogListing `json:"catalog"`
}

func NewMarket(profile *Profile) *Market {
	return &Market{
		Profile: profile, CatalogURL: ThemeCatalogURL, RawURL: ThemeRawURL,
		Client: &http.Client{Transport: netproxy.Transport(), CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }},
	}
}

func validDigest(value string) bool {
	if !strings.HasPrefix(value, "sha256:") || len(value) != 71 || strings.ToLower(value) != value {
		return false
	}
	_, err := hex.DecodeString(value[7:])
	return err == nil
}

func validCommit(value string) bool {
	if len(value) != 40 || strings.ToLower(value) != value {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}

func (entry CatalogEntry) validate() error {
	if !validThemeID(entry.ID) || entry.Path != entry.ID || strings.TrimSpace(entry.Name) == "" || strings.TrimSpace(entry.Author) == "" || strings.TrimSpace(entry.License) == "" || !validDigest(entry.Digest) {
		return errors.New("theme catalog identity is invalid")
	}
	if _, err := semver.StrictNewVersion(entry.Version); err != nil {
		return errors.New("theme catalog version is invalid")
	}
	if entry.Homepage != nil && themeHomepage(*entry.Homepage) != nil {
		return errors.New("theme catalog homepage is invalid")
	}
	seen, total := map[string]bool{}, 0
	for _, resource := range entry.Resources {
		limit := themeFileLimit
		if resource.Path == "theme.json" {
			limit = themeManifestLimit
		} else if validateThemePath(resource.Path) != nil {
			return errors.New("theme catalog resource path is invalid")
		}
		total += resource.Bytes
		if seen[resource.Path] || resource.Bytes <= 0 || resource.Bytes > limit || total > themePackageLimit || !validDigest(resource.Digest) {
			return errors.New("theme catalog resource is invalid")
		}
		seen[resource.Path] = true
	}
	modes := map[string]bool{}
	for _, mode := range entry.Modes {
		if modes[mode] || mode != "light" && mode != "dark" {
			return errors.New("theme catalog mode is invalid")
		}
		modes[mode] = true
	}
	for mode, relative := range entry.Previews {
		if !modes[mode] || validateThemePath(relative) != nil {
			return errors.New("theme catalog preview is invalid")
		}
	}
	return nil
}

func (listing CatalogListing) validate(requireCommit bool) error {
	if listing.Version != 1 || requireCommit && !validCommit(listing.Commit) {
		return errors.New("theme catalog format or commit is invalid")
	}
	seen := map[string]bool{}
	for _, entry := range listing.Themes {
		if err := entry.validate(); err != nil {
			return err
		}
		if seen[entry.ID] {
			return errors.New("theme catalog identity is repeated")
		}
		seen[entry.ID] = true
	}
	return nil
}

func (market *Market) body(ctx context.Context, endpoint string, limit int) ([]byte, error) {
	operation, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	request, err := http.NewRequestWithContext(operation, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	request.Header.Set("User-Agent", "Sailry/0.1")
	request.Header.Del("Referer")
	response, err := market.Client.Do(request)
	if err != nil {
		return nil, &themeSourceUnavailable{cause: err}
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, &themeSourceUnavailable{cause: errors.New("theme catalog source is unavailable")}
	}
	if response.ContentLength > int64(limit) {
		return nil, errors.New("theme resource exceeds its byte limit")
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, int64(limit+1)))
	if err != nil {
		return nil, &themeSourceUnavailable{cause: err}
	}
	if len(body) > limit {
		return nil, errors.New("theme resource exceeds its byte limit")
	}
	return body, nil
}

func (market *Market) List(ctx context.Context, force bool) (CatalogListing, error) {
	market.mu.Lock()
	defer market.mu.Unlock()
	if !force && market.cached != nil && time.Now().Before(market.until) {
		return *market.cached, nil
	}
	body, err := market.body(ctx, market.CatalogURL, protocol.MAX_FRAME_BYTES)
	var listing CatalogListing
	if err == nil {
		if json.Unmarshal(body, &listing) != nil || listing.validate(true) != nil {
			return listing, errors.New("theme catalog is invalid")
		}
		if err := market.save(listing); err != nil {
			return listing, err
		}
	} else {
		if ctx.Err() != nil {
			return listing, ctx.Err()
		}
		var unavailable *themeSourceUnavailable
		if !errors.As(err, &unavailable) {
			return listing, err
		}
		if market.cached != nil {
			listing = *market.cached
		} else if saved, readErr := market.restore(); readErr == nil {
			listing = saved
		} else {
			bundled, readErr := sailry.Themes.ReadFile("themes/catalog.json")
			if readErr != nil || json.Unmarshal(bundled, &listing) != nil || listing.validate(false) != nil {
				return listing, errors.New("bundled theme catalog is invalid")
			}
			listing.Commit = ""
		}
		listing.Unavailable = true
	}
	market.cached = &listing
	lifetime := 15 * time.Minute
	if listing.Unavailable {
		lifetime = 30 * time.Second
	}
	market.until = time.Now().Add(lifetime)
	return listing, nil
}

func (market *Market) restore() (CatalogListing, error) {
	root, err := os.OpenRoot(market.Profile.Root)
	if err != nil {
		return CatalogListing{}, err
	}
	defer root.Close()
	for _, name := range []string{".market", ".market/catalog.json"} {
		info, err := root.Lstat(name)
		if err != nil {
			return CatalogListing{}, err
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return CatalogListing{}, errors.New("theme catalog cache cannot use symbolic links")
		}
	}
	file, err := root.Open(".market/catalog.json")
	if err != nil {
		return CatalogListing{}, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() > protocol.MAX_FRAME_BYTES*2 {
		return CatalogListing{}, errors.New("theme catalog cache exceeds its byte limit")
	}
	body, err := io.ReadAll(io.LimitReader(file, protocol.MAX_FRAME_BYTES*2+1))
	if err != nil || len(body) > protocol.MAX_FRAME_BYTES*2 {
		return CatalogListing{}, errors.New("theme catalog cache exceeds its byte limit")
	}
	var saved savedCatalog
	if json.Unmarshal(body, &saved) != nil || saved.Version != 1 || !validCommit(saved.Commit) {
		return CatalogListing{}, errors.New("theme catalog cache is invalid")
	}
	saved.Catalog.Commit = saved.Commit
	return saved.Catalog, saved.Catalog.validate(true)
}

func (market *Market) save(listing CatalogListing) error {
	directory := filepath.Join(market.Profile.Root, ".market")
	if err := os.MkdirAll(directory, 0700); err != nil {
		return err
	}
	info, err := os.Lstat(directory)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return errors.New("theme catalog cache is not an ordinary directory")
	}
	catalog := listing
	catalog.Commit = ""
	body, err := json.Marshal(savedCatalog{Version: 1, Commit: listing.Commit, Catalog: catalog})
	if err != nil {
		return err
	}
	file, err := os.CreateTemp(directory, ".catalog-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	if _, err := file.Write(body); err != nil {
		_ = file.Close()
		return err
	}
	if err := errors.Join(file.Sync(), file.Close()); err != nil {
		return err
	}
	return os.Rename(file.Name(), filepath.Join(directory, "catalog.json"))
}

func (listing CatalogListing) Selection(id string) *ThemeSelection {
	if !validCommit(listing.Commit) {
		return nil
	}
	for _, entry := range listing.Themes {
		if entry.ID == id && entry.validate() == nil {
			return &ThemeSelection{Commit: listing.Commit, Entry: entry}
		}
	}
	return nil
}

func (entry CatalogEntry) Newer(current *Package) bool {
	before, beforeErr := semver.StrictNewVersion(current.Release)
	after, afterErr := semver.StrictNewVersion(entry.Version)
	return current.ID == entry.ID && beforeErr == nil && afterErr == nil && after.Compare(before) > 0
}

func (market *Market) Download(ctx context.Context, selection ThemeSelection) (Snapshot, error) {
	if !validCommit(selection.Commit) || selection.Entry.validate() != nil {
		return Snapshot{}, errors.New("theme selection is invalid")
	}
	resource := func(name string, limit int) ([]byte, error) {
		var descriptor *CatalogResource
		for index := range selection.Entry.Resources {
			if selection.Entry.Resources[index].Path == name {
				descriptor = &selection.Entry.Resources[index]
				limit = min(limit, descriptor.Bytes)
				break
			}
		}
		endpoint := strings.TrimRight(market.RawURL, "/") + "/" + selection.Commit + "/" + url.PathEscape(selection.Entry.Path) + "/" + name
		body, err := market.body(ctx, endpoint, limit)
		if err != nil {
			return nil, err
		}
		if descriptor != nil {
			digest := sha256.Sum256(body)
			if len(body) != descriptor.Bytes || "sha256:"+hex.EncodeToString(digest[:]) != descriptor.Digest {
				return nil, errors.New("theme resource integrity does not match its selection")
			}
		}
		return body, nil
	}
	manifest, err := resource("theme.json", themeManifestLimit)
	if err != nil {
		return Snapshot{}, err
	}
	var declaration Package
	if json.Unmarshal(manifest, &declaration) != nil || declaration.validate() != nil || declaration.ID != selection.Entry.ID || declaration.Release != selection.Entry.Version {
		return Snapshot{}, errors.New("theme manifest does not match its selection")
	}
	names := resourceNames(&declaration)
	for _, descriptor := range selection.Entry.Resources {
		if !slices.Contains(names, descriptor.Path) {
			return Snapshot{}, errors.New("theme catalog contains an undeclared resource")
		}
	}
	files, total := map[string][]byte{"theme.json": manifest}, len(manifest)
	for _, name := range names {
		if name == "theme.json" {
			continue
		}
		body, err := resource(name, min(themeFileLimit, themePackageLimit-total))
		if err != nil {
			return Snapshot{}, err
		}
		total += len(body)
		files[name] = body
	}
	snapshot, err := snapshotFiles(files)
	if err != nil || snapshot.Digest != selection.Entry.Digest {
		return Snapshot{}, errors.New("theme package integrity does not match its selection")
	}
	source, _ := json.Marshal(map[string]any{
		"version": 1, "repository": "https://github.com/sailry/sailry-themes",
		"commit": selection.Commit, "package_version": selection.Entry.Version, "digest": selection.Entry.Digest,
	})
	snapshot.Files[themeSourceFile] = source
	return snapshot, nil
}

func BundledSnapshot(ctx context.Context, id string) (Snapshot, error) {
	if !validThemeID(id) {
		return Snapshot{}, errors.New("invalid bundled theme ID")
	}
	tree, err := fs.Sub(sailry.Themes, "themes/"+id)
	if err != nil {
		return Snapshot{}, err
	}
	manifest, err := fs.ReadFile(tree, "theme.json")
	if err != nil {
		return Snapshot{}, err
	}
	var declaration Package
	if json.Unmarshal(manifest, &declaration) != nil || declaration.validate() != nil {
		return Snapshot{}, errors.New("bundled theme manifest is invalid")
	}
	files := map[string][]byte{}
	for _, name := range resourceNames(&declaration) {
		if err := ctx.Err(); err != nil {
			return Snapshot{}, err
		}
		body, err := fs.ReadFile(tree, path.Clean(name))
		if err != nil {
			return Snapshot{}, err
		}
		files[name] = body
	}
	return snapshotFiles(files)
}
