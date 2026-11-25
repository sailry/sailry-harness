//go:build integration && darwin

package updater

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Product inputs are explicit here: Harness must not import its downstream
// application. Code's own tests bind these values to its real Desktop identity.
func codeUpdateProductFixture() identity.Desktop {
	return identity.Desktop{
		ID: "code", Name: "Sailry Code", Version: "0.1.0", DefaultProfile: "~/.sailry",
		Release: identity.DesktopRelease{
			Source:    "https://api.github.com/repos/sailry/sailry-code/releases",
			Downloads: "https://github.com/sailry/sailry-code/releases/download",
			Package:   identity.DesktopPackage{Name: "Sailry", ID: "ai.sailry.desktop", Executable: "sailry-desktop"},
		},
	}
}

type productUpdateFeed struct {
	product identity.Desktop
	private ed25519.PrivateKey
	server  *httptest.Server
	mu      sync.Mutex
	files   map[string][]byte
	listing []listedRelease
	hits    map[string]int
	blocked []string
}

func newProductUpdateFeed(t *testing.T, product identity.Desktop) *productUpdateFeed {
	t.Helper()
	// This deterministic key exists only in the fixture, never in a release.
	private := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{61}, ed25519.SeedSize))
	product.Release.PublicKeys = base64.StdEncoding.EncodeToString(private.Public().(ed25519.PublicKey))
	f := &productUpdateFeed{product: product, private: private, files: map[string][]byte{}, hits: map[string]int{}}
	f.server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		f.hits[r.URL.Path]++
		if r.URL.Path == f.sourcePath() {
			if r.Method != http.MethodGet || r.URL.Query().Get("per_page") != "100" || r.URL.Query().Get("page") != "1" {
				http.Error(w, "unexpected release discovery request", http.StatusBadRequest)
				return
			}
			_ = json.NewEncoder(w).Encode(f.listing)
			return
		}
		data, ok := f.files[r.URL.Path]
		if !ok {
			http.NotFound(w, r)
			return
		}
		_, _ = w.Write(data)
	}))
	t.Cleanup(f.server.Close)
	f.prepare(t, private, product.Release.Package)
	return f
}

func (f *productUpdateFeed) sourcePath() string {
	address, _ := url.Parse(f.product.Release.Source)
	return address.Path
}

func (f *productUpdateFeed) assetPath(name string) string {
	address, _ := url.Parse(f.product.Release.Downloads)
	return address.Path + "/v0.1.1/" + name
}

// prepare builds an in-memory GitHub release using the existing signed catalog,
// archive and bundle fixtures. Nothing is published or read from a user profile.
func (f *productUpdateFeed) prepare(t *testing.T, archiveKey ed25519.PrivateKey, contents identity.DesktopPackage) {
	t.Helper()
	f.mu.Lock()
	defer f.mu.Unlock()
	listed := listedRelease{TagName: "v0.1.1"}
	add := func(name string, size uint64) {
		listed.Assets = append(listed.Assets, listedAsset{Name: name, State: "uploaded", Size: size,
			BrowserDownloadURL: f.product.Release.Downloads + "/v0.1.1/" + name})
	}
	var signed payload
	for _, target := range []string{"aarch64-apple-darwin", "x86_64-apple-darwin"} {
		name := f.product.Release.Package.Name + "-" + target + ".zip"
		archive := signArchive(t, bundleArchiveFor(t, target, "0.1.1", "13.0", contents), name, archiveKey)
		digest := sha256.Sum256(archive)
		entry := Release{Version: "0.1.1", Target: target, MinimumSystem: "13.0", Name: name,
			URL: f.product.Release.Downloads + "/v0.1.1/" + name, SHA256: hex.EncodeToString(digest[:]), Size: uint64(len(archive)),
			Bundle: f.product.Release.Package.Name + ".app", Executable: "Contents/MacOS/" + f.product.Release.Package.Executable}
		signed.Releases = append(signed.Releases, entry)
		f.files[f.assetPath(name)] = archive
		metadata, err := json.Marshal(entry)
		if err != nil {
			t.Fatal(err)
		}
		f.files[f.assetPath(name+".update.json")] = metadata
		add(name, uint64(len(archive)))
		add(name+".update.json", uint64(len(metadata)))
		// Discovery requires these assets, but the updater installs the ZIP.
		add(f.product.Release.Package.Name+"-"+target+".dmg", 1)
		add("SHA256SUMS-desktop-"+target, 1)
	}
	catalog := signCatalog(t, signed, f.private)
	f.files[f.assetPath("update-v1.json")] = catalog
	add("update-v1.json", uint64(len(catalog)))
	f.listing = []listedRelease{listed}
}

type productUpdateTransport struct{ feed *productUpdateFeed }

func (transport productUpdateTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	f := transport.feed
	address := request.URL
	allowed := address.Scheme == "https" && address.User == nil && address.Fragment == "" &&
		((address.Host == "api.github.com" && address.Path == f.sourcePath()) ||
			(address.Host == "github.com" && strings.HasPrefix(address.Path, f.assetPath(""))))
	if !allowed {
		f.mu.Lock()
		f.blocked = append(f.blocked, address.String())
		f.mu.Unlock()
		return nil, fmt.Errorf("fixture refuses non-Code request: %s", address.Redacted())
	}
	// Preserve production Code URLs through selection and authentication. Only
	// the transport changes, and it cannot dial anything except this TLS server.
	local, _ := url.Parse(f.server.URL)
	copy := request.Clone(request.Context())
	copy.URL.Scheme, copy.URL.Host, copy.Host = local.Scheme, local.Host, local.Host
	return f.server.Client().Transport.RoundTrip(copy)
}

func (f *productUpdateFeed) hit(path string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.hits[path]
}

func (f *productUpdateFeed) assertIsolated(t *testing.T) {
	t.Helper()
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.blocked) != 0 {
		t.Fatalf("updater attempted requests outside its Code fixture: %v", f.blocked)
	}
}

type productUpdateHandoff struct {
	stops, exchanges, quits int
	executable              string
	arguments               []string
	order                   []string
}

type productUpdateRun struct {
	feed                     *productUpdateFeed
	service                  *Service
	calls                    *productUpdateHandoff
	installed, profile, root string
	hostProfile              string
	before, hostBefore       map[string]productUpdateFile
}

func newProductUpdateRun(t *testing.T, product identity.Desktop, target string, override bool) *productUpdateRun {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	dataOverride := ""
	if override {
		dataOverride = filepath.Join(home, "explicit-data")
	}
	profile, err := product.DataDir(dataOverride)
	if err != nil {
		t.Fatal(err)
	}
	f := newProductUpdateFeed(t, product)
	r := &productUpdateRun{feed: f, calls: &productUpdateHandoff{}, profile: profile,
		installed: filepath.Join(home, "Applications", product.Release.Package.Name+".app"),
		root:      filepath.Join(profile, "desktop", "updater"), hostProfile: filepath.Join(home, ".sailry-host")}
	files := bundleFiles(target, product.Version, "13.0")
	files["Contents/Info.plist"] = []byte(strings.NewReplacer("ai.sailry.desktop", product.Release.Package.ID,
		"sailry-desktop", product.Release.Package.Executable).Replace(string(files["Contents/Info.plist"])))
	files["Contents/MacOS/"+product.Release.Package.Executable] = files["Contents/MacOS/sailry-desktop"]
	if product.Release.Package.Executable != "sailry-desktop" {
		delete(files, "Contents/MacOS/sailry-desktop")
	}
	files["Contents/Resources/old-version-only.txt"] = []byte("original 0.1.0 resource")
	writeProductUpdateFiles(t, r.installed, files, 0o755)
	seed := bytes.Repeat([]byte{81}, ed25519.SeedSize)
	peer := protocol.NodeID(ed25519.NewKeyFromSeed(bytes.Repeat([]byte{82}, ed25519.SeedSize)).Public().(ed25519.PublicKey))
	paired, err := json.Marshal(map[string]any{"version": 1, "peers": []any{map[string]any{
		"id": peer, "name": "Fixture Host", "address": nil, "direction": "host", "direct": true,
		"direct_direction": "host", "my_device": true, "removed": false, "directory_revision": 1, "has_node": true,
	}}})
	if err != nil {
		t.Fatal(err)
	}
	// These are opaque data-retention sentinels, not a live Node or a database
	// migration test. Pairing and identity sentinels mirror persisted file formats.
	writeProductUpdateFiles(t, profile, map[string][]byte{
		"link.key":                           seed,
		"storage/node.sqlite":                []byte("isolated session and configuration database\x00\x01"),
		"storage/node.sqlite-wal":            []byte("isolated pending database pages\x00\x02"),
		"credentials/fixture.json":           []byte(`{"provider":"fixture","token":"not-a-real-credential"}`),
		"desktop/controller/link.key":        seed,
		"desktop/controller/peers.json":      paired,
		"desktop/drafts/fixture.txt":         []byte("independently owned unsaved draft"),
		"desktop/preferences.json":           []byte(`{"version":1,"notifications":[true,true,true,true],"toast_seconds":5,"pairing":"https://pairing.invalid/","terminal":{"font_size":14,"font_family":"","paste_protection":true},"appearance":{"package":"builtin","mode":"system"},"update_channel":"stable"}`),
		"projects/fixture/retained-file.txt": []byte("user project contents"),
	}, 0o600)
	writeProductUpdateFiles(t, r.hostProfile, map[string][]byte{
		"link.key": seed, "storage/node.sqlite": []byte("existing Host sessions and pairing"),
	}, 0o600)
	r.before = snapshotProductUpdateData(t, r.profile, r.root)
	r.hostBefore = snapshotProductUpdateData(t, r.hostProfile, "")
	previousArgs := os.Args
	os.Args = []string{filepath.Join(r.installed, "Contents", "MacOS", product.Release.Package.Executable)}
	if override {
		os.Args = append(os.Args, "--data-dir", profile)
	}
	t.Cleanup(func() { os.Args = previousArgs })
	r.service = New(Options{Version: product.Version, Release: f.product.Release, Directory: r.root, Profile: profile,
		Stop: func() error {
			r.calls.stops++
			r.calls.order = append(r.calls.order, "stop")
			return nil
		}, Quit: func() {
			r.calls.quits++
			r.calls.order = append(r.calls.order, "quit")
		}})
	// Keep New's product release binding; only OS version and target are fixed.
	release := r.service.release
	r.service.release = func(channel Channel, version string) (Config, error) {
		config, err := release(channel, version)
		config.Target, config.System = target, "14.2"
		return config, err
	}
	r.service.client = func(config *Config) *http.Client {
		client := newClient(config)
		client.Transport = productUpdateTransport{feed: f}
		return client
	}
	r.service.installed = func() (string, error) { return r.installed, nil }
	r.service.exchange = func(staged, installed string) error {
		r.calls.exchanges++
		r.calls.order = append(r.calls.order, "exchange")
		return exchangeFixtureBundles(staged, installed)
	}
	r.service.relaunch = func(executable string, arguments []string) error {
		r.calls.executable, r.calls.arguments = executable, append([]string(nil), arguments...)
		r.calls.order = append(r.calls.order, "relaunch")
		return nil
	}
	t.Cleanup(r.service.Close)
	return r
}

func writeProductUpdateFiles(t *testing.T, root string, files map[string][]byte, mode fs.FileMode) {
	t.Helper()
	for name, data := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, data, mode); err != nil {
			t.Fatal(err)
		}
	}
}

type productUpdateFile struct {
	Mode     fs.FileMode
	Contents string
	Modified int64
}

func snapshotProductUpdateData(t *testing.T, root, cache string) map[string]productUpdateFile {
	t.Helper()
	files := map[string]productUpdateFile{}
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if path == cache {
			return filepath.SkipDir
		}
		name, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		value := productUpdateFile{Mode: info.Mode()}
		if !entry.IsDir() {
			data, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			value.Contents, value.Modified = string(data), info.ModTime().UnixNano()
		}
		files[name] = value
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return files
}

func (r *productUpdateRun) assertDataUnchanged(t *testing.T) {
	t.Helper()
	if after := snapshotProductUpdateData(t, r.profile, r.root); !reflect.DeepEqual(after, r.before) {
		t.Fatal("update changed the data profile outside its own cache (paths, bytes, permissions or file modification times)")
	}
	if after := snapshotProductUpdateData(t, r.hostProfile, ""); !reflect.DeepEqual(after, r.hostBefore) {
		t.Fatal("desktop update changed the separate Host profile or pairing")
	}
	r.feed.assertIsolated(t)
}

func productUpdateBundleVersion(t *testing.T, bundle string) string {
	t.Helper()
	version, err := FixtureBundleVersion(bundle)
	if err != nil {
		t.Fatal(err)
	}
	return version
}
