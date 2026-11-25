package theme

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
)

func profileFixture(t *testing.T, id, version string) Snapshot {
	t.Helper()
	manifest, assets := themeFixture(t)
	manifest["id"], manifest["package_version"] = id, version
	data, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	files := map[string][]byte{"theme.json": data}
	for name, file := range assets {
		files[name] = append([]byte(nil), file.Data...)
	}
	snapshot, err := snapshotFiles(files)
	if err != nil {
		t.Fatal(err)
	}
	return snapshot
}

func TestThemeProfileKeepsExistingInstallations(t *testing.T) {
	defer SetInstalled(nil)
	ctx := t.Context()
	profile := NewProfile(filepath.Join(t.TempDir(), "desktop"))
	before := profileFixture(t, "ink-dragon", "0.1.0")
	for name, body := range before.Files {
		filename := filepath.Join(profile.Root, before.Package.ID, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, body, 0600); err != nil {
			t.Fatal(err)
		}
	}
	installed, err := profile.Installed(ctx)
	if err != nil || len(installed) != 1 {
		t.Fatalf("existing themes = %d, %v", len(installed), err)
	}
	pkg := installed[0].Package
	SetInstalled([]*Package{pkg})
	if Find(before.Package.ID) != pkg || Look(pkg.ID, false).Variant != pkg.Light || Look(pkg.ID, true).Variant != pkg.Dark {
		t.Fatal("the existing selected theme was not registered")
	}
	if pkg.Light.Asset("background.content") == nil || pkg.Dark.Asset("background.content") == nil {
		t.Fatal("the existing theme lost its artwork")
	}
	after, err := ReadSnapshot(ctx, filepath.Join(profile.Root, pkg.ID))
	if err != nil || after.Digest != before.Digest {
		t.Fatalf("reading the existing theme changed its files: %v", err)
	}
	entries, err := os.ReadDir(profile.Root)
	if err != nil || len(entries) != 1 || entries[0].Name() != pkg.ID {
		t.Fatalf("reading existing themes changed the profile: %v, %v", entries, err)
	}
}

func TestThemeProfilePublication(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	profile := NewProfile(t.TempDir())
	first, other := profileFixture(t, "coast", "0.1.0"), profileFixture(t, "forest", "0.1.0")
	if err := profile.Install(ctx, first, nil); err != nil {
		t.Fatal(err)
	}
	if err := profile.Install(ctx, other, nil); err != nil {
		t.Fatal(err)
	}
	exported := filepath.Join(t.TempDir(), "coast.zip")
	if err := first.Export(exported); err != nil {
		t.Fatal(err)
	}
	reloaded, err := ReadSnapshot(ctx, exported)
	if err != nil || reloaded.Digest != first.Digest {
		t.Fatalf("exported snapshot = %s, %v", reloaded.Digest, err)
	}
	next := profileFixture(t, "coast", "0.1.1")
	wrong := other.Digest
	if err := profile.Install(ctx, next, &wrong); err == nil {
		t.Fatal("conflicting update replaced a theme")
	}
	stored, err := ReadSnapshot(ctx, filepath.Join(profile.Root, "coast"))
	if err != nil || stored.Digest != first.Digest {
		t.Fatalf("failed update changed installed theme: %v", err)
	}
	if err := profile.Install(ctx, next, &first.Digest); err != nil {
		t.Fatal(err)
	}
	if err := profile.Remove(ctx, "coast", first.Digest); err == nil {
		t.Fatal("stale removal succeeded")
	}
	if err := profile.Remove(ctx, "coast", next.Digest); err != nil {
		t.Fatal(err)
	}
	installed, err := profile.Installed(ctx)
	if err != nil || len(installed) != 1 || installed[0].Package.ID != "forest" {
		t.Fatalf("uninstall affected another package: %v", err)
	}
	// A directory that is not its theme is left out and named; the valid
	// theme beside it still comes back.
	if err := os.Mkdir(filepath.Join(profile.Root, "broken"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := profile.Install(ctx, first, nil); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(filepath.Join(profile.Root, "coast"), filepath.Join(profile.Root, "renamed")); err != nil {
		t.Fatal(err)
	}
	installed, err = profile.Installed(ctx)
	if len(installed) != 1 || installed[0].Package.ID != "forest" {
		t.Fatalf("an invalid theme changed the installed list: %d themes, %v", len(installed), err)
	}
	if err == nil || !strings.Contains(err.Error(), "installed theme broken") || !strings.Contains(err.Error(), "installed theme renamed") {
		t.Fatalf("invalid themes not named: %v", err)
	}
	for _, name := range []string{"broken", "renamed"} {
		if err := os.RemoveAll(filepath.Join(profile.Root, name)); err != nil {
			t.Fatal(err)
		}
	}
	if runtime.GOOS != "windows" {
		outside := filepath.Join(t.TempDir(), "outside")
		if err := os.Mkdir(outside, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(outside, filepath.Join(profile.Root, "linked")); err != nil {
			t.Fatal(err)
		}
		if _, err := ReadSnapshot(ctx, filepath.Join(profile.Root, "linked")); err == nil {
			t.Fatal("symbolic link theme source was accepted")
		}
	}
}

func TestThemeMarketImmutableSelection(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	snapshot := profileFixture(t, "coast", "0.1.0")
	commit := strings.Repeat("a", 40)
	entry := CatalogEntry{ID: "coast", Path: "coast", Name: "Coast", Version: "0.1.0", Author: "Sailry", License: "CC0-1.0", Digest: snapshot.Digest, Modes: []string{"light", "dark"}}
	for _, name := range resourceNames(snapshot.Package) {
		sum := sha256.Sum256(snapshot.Files[name])
		entry.Resources = append(entry.Resources, CatalogResource{Path: name, Bytes: len(snapshot.Files[name]), Digest: "sha256:" + hex.EncodeToString(sum[:])})
	}
	listing := CatalogListing{Version: 1, Commit: commit, Themes: []CatalogEntry{entry}}
	var corruptFeed, corruptResource, unavailable atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path == "/catalog" {
			if unavailable.Load() {
				http.Error(writer, "unavailable", http.StatusServiceUnavailable)
				return
			}
			value := listing
			if corruptFeed.Load() {
				value.Commit = "main"
			}
			_ = json.NewEncoder(writer).Encode(value)
			return
		}
		prefix := "/raw/" + commit + "/coast/"
		if !strings.HasPrefix(request.URL.Path, prefix) {
			http.NotFound(writer, request)
			return
		}
		name := strings.TrimPrefix(request.URL.Path, prefix)
		data, exists := snapshot.Files[name]
		if !exists {
			http.NotFound(writer, request)
			return
		}
		if corruptResource.Load() && name == "assets/light.png" {
			data = append([]byte(nil), data...)
			data[0] ^= 1
		}
		_, _ = writer.Write(data)
	}))
	defer server.Close()
	profile := NewProfile(t.TempDir())
	market := NewMarket(profile)
	market.Client, market.CatalogURL, market.RawURL = server.Client(), server.URL+"/catalog", server.URL+"/raw/"
	got, err := market.List(ctx, false)
	if err != nil || got.Selection("coast") == nil {
		t.Fatalf("bound catalog: %v", err)
	}
	downloaded, err := market.Download(ctx, *got.Selection("coast"))
	if err != nil || downloaded.Digest != snapshot.Digest {
		t.Fatalf("immutable download: %v", err)
	}
	if err := profile.Install(ctx, downloaded, nil); err != nil {
		t.Fatal(err)
	}
	corruptFeed.Store(true)
	if _, err := market.List(ctx, true); err == nil {
		t.Fatal("an unbound catalog replaced the cache")
	}
	if market.cached.Commit != commit {
		t.Fatal("invalid catalog changed the retained commit")
	}
	corruptResource.Store(true)
	if _, err := market.Download(ctx, *got.Selection("coast")); err == nil {
		t.Fatal("corrupt package download succeeded")
	}
	installed, err := profile.Installed(ctx)
	if err != nil || len(installed) != 1 || installed[0].Digest != snapshot.Digest {
		t.Fatalf("failed download changed the installed version: %v", err)
	}
	unavailable.Store(true)
	restarted := NewMarket(profile)
	restarted.Client, restarted.CatalogURL = server.Client(), server.URL+"/catalog"
	for _, retained := range []*Market{market, restarted} {
		cached, err := retained.List(ctx, true)
		if err != nil || !cached.Unavailable || cached.Commit != commit || cached.Selection("coast") == nil {
			t.Fatalf("unavailable catalog did not retain its immutable selection: %+v, %v", cached, err)
		}
	}
}
