package main

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

const (
	fixtureRelayTag      = "v2.10.0"
	fixtureRelayLatest   = "/releases/latest"
	fixtureRelayBase     = "/releases/download"
	fixtureRelayDownload = fixtureRelayBase + "/" + fixtureRelayTag + "/"
	fixtureRelayArchive  = "sailry-relay-linux-amd64.tar.gz"
	fixtureRelayChecksum = "SHA256SUMS-relay-linux-amd64"
)

func TestRelayReleaseDoesNotDownloadSameOrOlderVersion(t *testing.T) {
	for _, current := range []string{"2.10.0", "v2.10.0+local", "2.11.0"} {
		t.Run(current, func(t *testing.T) {
			metadata := relayFixtureMetadata(fixtureRelayTag, fixtureRelayArchive, fixtureRelayChecksum)
			source := newRelayReleaseFixture(t, metadata, nil)
			stage := t.TempDir()
			path, version, err := source.latest(t.Context(), current, "amd64", stage)
			if err != nil || path != "" || version != "2.10.0" {
				t.Fatalf("latest = (%q, %q, %v), want no update and version 2.10.0", path, version, err)
			}
			assertRelayStage(t, stage)
		})
	}
}

func TestRelayReleaseStagesVerifiedBinaryForTarget(t *testing.T) {
	for _, arch := range []string{"amd64", "arm64"} {
		t.Run(arch, func(t *testing.T) {
			const binary = "fixture relay binary"
			data := relayFixtureTar(t,
				relayFixtureMember{name: "./", kind: tar.TypeDir},
				relayFixtureMember{name: "./sailry-relay", body: binary},
				relayFixtureMember{name: "./licenses/", kind: tar.TypeDir},
				relayFixtureMember{name: "./licenses/NOTICE", body: "not extracted"},
			)
			archive := "sailry-relay-linux-" + arch + ".tar.gz"
			checksum := "SHA256SUMS-relay-linux-" + arch
			metadata := relayFixtureMetadata(fixtureRelayTag, archive, checksum)
			sums := fmt.Sprintf("%x *./%s\r\n", sha256.Sum256(data), archive)
			source := newRelayReleaseFixture(t, metadata, map[string][]byte{
				fixtureRelayDownload + archive:  data,
				fixtureRelayDownload + checksum: []byte(sums),
			})
			stage := t.TempDir()
			path, version, err := source.latest(t.Context(), "2.9.0", arch, stage)
			if err != nil || version != "2.10.0" || path != filepath.Join(stage, "sailry-relay") {
				t.Fatalf("latest = (%q, %q, %v), want staged binary for 2.10.0", path, version, err)
			}
			got, err := os.ReadFile(path)
			if err != nil || string(got) != binary {
				t.Fatalf("staged binary = %q, %v; want %q", got, err, binary)
			}
			assertRelayStage(t, stage, "sailry-relay")
		})
	}
}

func TestRelayReleaseRejectsInvalidMetadataOrChecksum(t *testing.T) {
	data := relayFixtureTar(t, relayFixtureMember{name: "sailry-relay", body: "relay"})
	valid := relayFixtureMetadata(fixtureRelayTag, fixtureRelayArchive, fixtureRelayChecksum)
	correct := fmt.Sprintf("%x  %s\n", sha256.Sum256(data), fixtureRelayArchive)
	for _, row := range []struct {
		name, metadata, sums, wantError string
		archive                         []byte
	}{
		{name: "missing assets", metadata: relayFixtureMetadata(fixtureRelayTag), wantError: "found 0"},
		{name: "duplicate asset", metadata: relayFixtureMetadata(fixtureRelayTag, fixtureRelayArchive, fixtureRelayArchive, fixtureRelayChecksum), wantError: "found 2"},
		{name: "draft", metadata: `{"tag_name":"v2.10.0","draft":true}`, wantError: "not a stable version tag"},
		{name: "tag with build metadata", metadata: relayFixtureMetadata("v2.10.0+replacement"), wantError: "not a stable version tag"},
		{name: "prerelease tag", metadata: relayFixtureMetadata("v2.10.0-rc.1"), wantError: "not a stable version tag"},
		{name: "malformed tag", metadata: relayFixtureMetadata("v2.10"), wantError: "not a stable version tag"},
		{name: "duplicate checksum", metadata: valid, sums: correct + correct, wantError: "duplicate SHA-256"},
		{name: "wrong checksum name", metadata: valid, sums: strings.ReplaceAll(correct, "amd64", "arm64"), wantError: "missing SHA-256"},
		{name: "hash mismatch", metadata: valid, sums: fmt.Sprintf("%064x  %s\n", 0, fixtureRelayArchive), archive: data, wantError: "checksum does not match"},
	} {
		t.Run(row.name, func(t *testing.T) {
			assets := make(map[string][]byte)
			if row.sums != "" {
				assets[fixtureRelayDownload+fixtureRelayChecksum] = []byte(row.sums)
			}
			if row.archive != nil {
				assets[fixtureRelayDownload+fixtureRelayArchive] = row.archive
			}
			source := newRelayReleaseFixture(t, row.metadata, assets)
			stage := t.TempDir()
			path, _, err := source.latest(t.Context(), "2.9.0", "amd64", stage)
			if path != "" || err == nil || !strings.Contains(err.Error(), row.wantError) {
				t.Fatalf("latest path = %q, error = %v; want %q", path, err, row.wantError)
			}
			assertRelayStage(t, stage)
		})
	}
}

func TestRelayReleaseRejectsUnsafeArchiveMembers(t *testing.T) {
	binary := relayFixtureMember{name: "./sailry-relay", body: "new relay"}
	for _, row := range []struct {
		name    string
		members []relayFixtureMember
	}{
		{name: "only nested", members: []relayFixtureMember{{name: "licenses/sailry-relay", body: "decoy"}}},
		{name: "duplicate", members: []relayFixtureMember{binary, {name: "sailry-relay", body: "another relay"}}},
		{name: "symlink", members: []relayFixtureMember{{name: "sailry-relay", kind: tar.TypeSymlink, link: "../installed"}}},
		{name: "traversal", members: []relayFixtureMember{binary, {name: "../installed", body: "must not escape"}}},
	} {
		t.Run(row.name, func(t *testing.T) {
			root := t.TempDir()
			stage := filepath.Join(root, "stage")
			if err := os.Mkdir(stage, 0o700); err != nil {
				t.Fatal(err)
			}
			installed := filepath.Join(root, "installed")
			if err := os.WriteFile(installed, []byte("previous relay"), 0o700); err != nil {
				t.Fatal(err)
			}
			source := newRelayReleaseArchiveFixture(t, relayFixtureTar(t, row.members...))
			if path, _, err := source.latest(t.Context(), "2.9.0", "amd64", stage); path != "" || err == nil {
				t.Fatalf("latest accepted unsafe archive: %q, %v", path, err)
			}
			assertRelayStage(t, stage)
			if got, err := os.ReadFile(installed); err != nil || string(got) != "previous relay" {
				t.Fatalf("installed file changed: %q, %v", got, err)
			}
		})
	}
}

func TestRelayReleaseDoesNotFollowStagedBinarySymlink(t *testing.T) {
	root := t.TempDir()
	stage := filepath.Join(root, "stage")
	if err := os.Mkdir(stage, 0o700); err != nil {
		t.Fatal(err)
	}
	installed := filepath.Join(root, "installed")
	if err := os.WriteFile(installed, []byte("previous relay"), 0o700); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(stage, "sailry-relay")
	if err := os.Symlink(installed, link); err != nil {
		if runtime.GOOS == "windows" {
			t.Skipf("fixture symlink unavailable: %v", err)
		}
		t.Fatal(err)
	}
	source := newRelayReleaseArchiveFixture(t, relayFixtureTar(t, relayFixtureMember{name: "sailry-relay", body: "new relay"}))
	if path, _, err := source.latest(t.Context(), "2.9.0", "amd64", stage); path != "" || err == nil {
		t.Fatalf("latest succeeded with staged binary symlink: %q, %v", path, err)
	}
	if target, err := os.Readlink(link); err != nil || target != installed {
		t.Fatalf("staged symlink changed: %q, %v", target, err)
	}
	if got, err := os.ReadFile(installed); err != nil || string(got) != "previous relay" {
		t.Fatalf("symlink target changed: %q, %v", got, err)
	}
}

func TestRelayReleaseTransferRejectsOversizeResponses(t *testing.T) {
	for _, reported := range []bool{true, false} {
		t.Run(fmt.Sprintf("reported=%t", reported), func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				if reported {
					w.Header().Set("Content-Length", "8")
				} else {
					w.(http.Flusher).Flush()
				}
				_, _ = io.WriteString(w, "oversize")
			}))
			t.Cleanup(server.Close)
			source := relayReleases{client: server.Client()}
			var got bytes.Buffer
			if err := source.fetch(t.Context(), server.URL, 4, &got); err == nil || !strings.Contains(err.Error(), "4-byte limit") {
				t.Fatalf("oversize fetch error = %v", err)
			}
			if got.Len() > 5 || (reported && got.Len() != 0) {
				t.Fatalf("wrote %d bytes before rejecting oversize transfer", got.Len())
			}
		})
	}
}

func TestOfficialRelayReleaseRedirectAllowlist(t *testing.T) {
	source := officialRelayReleases()
	for _, row := range []struct {
		name, from, to string
		allow          bool
	}{
		{name: "CDN", to: "https://release-assets.githubusercontent.com/download?signature=fixture", allow: true},
		{name: "legacy CDN", to: "https://objects.githubusercontent.com/download", allow: true},
		{name: "wrong host", to: "https://release-assets.githubusercontent.com.invalid/download"},
		{name: "HTTP", to: "http://release-assets.githubusercontent.com/download"},
		{name: "credentials", to: "https://user:password@release-assets.githubusercontent.com/download"},
		{name: "port", to: "https://release-assets.githubusercontent.com:8443/download"},
		{name: "discovery redirect", from: source.latestURL, to: "https://release-assets.githubusercontent.com/download"},
	} {
		t.Run(row.name, func(t *testing.T) {
			from := row.from
			if from == "" {
				from = source.releaseBase + "/" + fixtureRelayTag + "/" + fixtureRelayArchive
			}
			prev, err := http.NewRequest(http.MethodGet, from, nil)
			if err != nil {
				t.Fatal(err)
			}
			next, err := http.NewRequest(http.MethodGet, row.to, nil)
			if err != nil {
				t.Fatal(err)
			}
			next.Header.Set("Referer", from)
			if err := source.client.CheckRedirect(next, []*http.Request{prev}); (err == nil) != row.allow {
				t.Fatalf("redirect allowed = %t, want %t; error = %v", err == nil, row.allow, err)
			}
			if row.allow && next.Header.Get("Referer") != "" {
				t.Fatal("release redirect retained the previous URL as referer")
			}
		})
	}
}

func newRelayReleaseFixture(t *testing.T, metadata string, assets map[string][]byte) relayReleases {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.RawQuery != "" || r.Header.Get("Authorization") != "" || r.Header.Get("Cookie") != "" {
			t.Error("release request must be an unauthenticated GET without a query")
		}
		if r.URL.Path == fixtureRelayLatest {
			_, _ = io.WriteString(w, strings.ReplaceAll(metadata, "fixture.invalid", r.Host))
			return
		}
		if data, ok := assets[r.URL.Path]; ok {
			_, _ = w.Write(data)
			return
		}
		t.Errorf("unexpected release request: %s", r.URL.Path)
		http.NotFound(w, r)
	}))
	t.Cleanup(server.Close)
	return relayReleases{client: server.Client(), latestURL: server.URL + fixtureRelayLatest, releaseBase: server.URL + fixtureRelayBase}
}

func newRelayReleaseArchiveFixture(t *testing.T, data []byte) relayReleases {
	t.Helper()
	metadata := relayFixtureMetadata(fixtureRelayTag, fixtureRelayArchive, fixtureRelayChecksum)
	sums := fmt.Sprintf("%x  %s\n", sha256.Sum256(data), fixtureRelayArchive)
	return newRelayReleaseFixture(t, metadata, map[string][]byte{
		fixtureRelayDownload + fixtureRelayArchive:  data,
		fixtureRelayDownload + fixtureRelayChecksum: []byte(sums),
	})
}

func relayFixtureMetadata(tag string, names ...string) string {
	assets := make([]string, 0, len(names))
	for _, name := range names {
		assets = append(assets, fmt.Sprintf(`{"name":%q,"browser_download_url":"http://fixture.invalid/untrusted"}`, name))
	}
	return fmt.Sprintf(`{"tag_name":%q,"assets":[%s]}`, tag, strings.Join(assets, ","))
}

type relayFixtureMember struct {
	name, body, link string
	kind             byte
}

func relayFixtureTar(t *testing.T, members ...relayFixtureMember) []byte {
	t.Helper()
	var data bytes.Buffer
	compressed := gzip.NewWriter(&data)
	archive := tar.NewWriter(compressed)
	for _, member := range members {
		header := tar.Header{Name: member.name, Mode: 0o755, Typeflag: member.kind, Linkname: member.link, Size: int64(len(member.body))}
		if err := archive.WriteHeader(&header); err != nil {
			t.Fatal(err)
		}
		if _, err := io.WriteString(archive, member.body); err != nil {
			t.Fatal(err)
		}
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	if err := compressed.Close(); err != nil {
		t.Fatal(err)
	}
	return data.Bytes()
}

func assertRelayStage(t *testing.T, stage string, want ...string) {
	t.Helper()
	entries, err := os.ReadDir(stage)
	if err != nil {
		t.Fatal(err)
	}
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	if strings.Join(names, ",") != strings.Join(want, ",") {
		t.Fatalf("staging files = %v, want %v", names, want)
	}
}
