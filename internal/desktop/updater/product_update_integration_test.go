//go:build integration && darwin

package updater

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

// TestCodeDesktopUpdate010To011 covers a complete macOS update with a local TLS
// GitHub fixture and a non-production publisher key. unsignedBundles disables
// only OS codesign verification; catalog/ZIP signatures, digest, package checks,
// same-volume extraction and Darwin's atomic exchange remain real. Relaunch and
// Node shutdown are recorded callbacks, not real processes or signed acceptance.
func TestCodeDesktopUpdate010To011(t *testing.T) {
	unsignedBundles(t)
	for _, target := range []string{"aarch64-apple-darwin", "x86_64-apple-darwin"} {
		for _, override := range []bool{false, true} {
			profile := "compatible-default-profile"
			if override {
				profile = "explicit-data-directory"
			}
			t.Run(target+"/"+profile, func(t *testing.T) {
				r := newProductUpdateRun(t, codeUpdateProductFixture(), target, override)
				checkProductUpdateAvailable(t, r)
				config := r.service.config
				if config.Source != r.feed.product.Release.Source || config.Downloads != r.feed.product.Release.Downloads ||
					config.Package != r.feed.product.Release.Package || config.Version != "0.1.0" || config.Channel != Stable ||
					len(config.Keys) != 1 || !bytes.Equal(config.Keys[0], r.feed.private.Public().(ed25519.PublicKey)) {
					t.Fatal("service lost the explicit product source, package or fixture trust binding")
				}
				r.assertDataUnchanged(t)
				downloadProductUpdate(t, r)
				stage := r.service.staged
				if stage == nil || productUpdateBundleVersion(t, filepath.Join(stage.directory, "contents", "Sailry.app")) != "0.1.1" ||
					productUpdateBundleVersion(t, r.installed) != "0.1.0" || r.calls.stops != 0 || r.calls.exchanges != 0 {
					t.Fatal("download did not stage 0.1.1 separately from the running 0.1.0 application")
				}
				if err := verifyArchive(stage.archive, stage.selection.Release, config.Keys); err != nil {
					t.Fatalf("staged archive was not authenticated: %v", err)
				}
				r.assertDataUnchanged(t)
				arguments := append([]string(nil), os.Args[1:]...)
				r.service.Install()
				if _, failure, notices := settle(r.service); failure != nil || len(notices) != 0 {
					t.Fatalf("install = %v, %+v", failure, notices)
				}
				if r.calls.stops != 1 || r.calls.exchanges != 1 || r.calls.quits != 1 ||
					r.calls.executable != filepath.Join(r.installed, "Contents", "MacOS", "sailry-desktop") ||
					!reflect.DeepEqual(r.calls.arguments, arguments) ||
					!reflect.DeepEqual(r.calls.order, []string{"stop", "exchange", "relaunch", "quit"}) {
					t.Fatalf("install/relaunch handoff = %+v", r.calls)
				}
				if productUpdateBundleVersion(t, r.installed) != "0.1.1" {
					t.Fatal("the installed application is not 0.1.1")
				}
				if _, err := os.Stat(filepath.Join(r.installed, "Contents", "Resources", "old-version-only.txt")); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("whole-application replacement retained an old-only resource: %v", err)
				}
				if destination, err := os.Readlink(filepath.Join(r.installed, "Contents", "Resources", "current.json")); err != nil || destination != "build.json" {
					t.Fatalf("installed resource symlink = %q, %v", destination, err)
				}
				backups, err := filepath.Glob(filepath.Join(filepath.Dir(r.installed), ".sailry-update-backup-*"))
				if err != nil || len(backups) != 1 || productUpdateBundleVersion(t, filepath.Join(backups[0], "Sailry.app")) != "0.1.0" {
					t.Fatalf("retained original = %v, %v", backups, err)
				}
				oldResource := filepath.Join(backups[0], "Sailry.app", "Contents", "Resources", "old-version-only.txt")
				if data, err := os.ReadFile(oldResource); err != nil || string(data) != "original 0.1.0 resource" {
					t.Fatalf("backup lost the original resource: %v", err)
				}
				r.assertDataUnchanged(t)

				// Startup acknowledges the same product/profile's handoff. Close
				// immediately joins receipt work and stops its periodic watcher.
				next := New(Options{Version: "0.1.1", Release: r.feed.product.Release, Directory: r.root, Profile: r.profile})
				next.installed = func() (string, error) { return r.installed, nil }
				next.client = r.service.client
				next.Startup()
				next.Close()
				if _, failure, notices := settle(next); failure != nil || len(notices) != 1 || notices[0].Key != "updates_completed" || notices[0].Error {
					t.Fatalf("0.1.1 receipt acknowledgement = %v, %+v", failure, notices)
				}
				if _, err := os.Stat(backups[0]); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("acknowledged original remains: %v", err)
				}
				if key, err := readReceipt(r.root, r.installed, "0.1.1", "Sailry.app"); key != "" || err != nil {
					t.Fatalf("receipt was acknowledged twice: %q, %v", key, err)
				}
				archived, err := filepath.Glob(filepath.Join(r.root, "receipts", "receipt-*.json"))
				if err != nil || len(archived) != 1 {
					t.Fatalf("archived receipt = %v, %v", archived, err)
				}
				if r.feed.hit(r.feed.sourcePath()) != 1 || r.feed.hit(r.feed.assetPath("update-v1.json")) != 1 ||
					r.feed.hit(r.feed.assetPath("Sailry-"+target+".zip")) != 1 {
					t.Fatal("check/download did not use the expected Code GitHub paths exactly once")
				}
				r.service.Close()
				r.assertDataUnchanged(t)
			})
		}
	}
}

func TestCodeDesktopUpdateRejectsUnauthenticatedOrMismatched011(t *testing.T) {
	unsignedBundles(t)
	foreign := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{62}, ed25519.SeedSize))
	for _, test := range []struct {
		name       string
		failure    string
		checkFails bool
		mutate     func(*testing.T, *productUpdateRun)
	}{
		{"untrusted catalog", "updates_signature_invalid", true, func(t *testing.T, r *productUpdateRun) {
			path := r.feed.assetPath("update-v1.json")
			var catalog envelope
			if err := json.Unmarshal(r.feed.files[path], &catalog); err != nil {
				t.Fatal(err)
			}
			catalog.Signature = base64.StdEncoding.EncodeToString(ed25519.Sign(foreign, catalog.Payload))
			data, err := json.Marshal(catalog)
			if err != nil {
				t.Fatal(err)
			}
			r.feed.files[path] = data
		}},
		{"untrusted archive with a trusted digest", "updates_signature_invalid", false, func(t *testing.T, r *productUpdateRun) {
			r.feed.prepare(t, foreign, r.feed.product.Release.Package)
		}},
		{"changed archive", "updates_digest_invalid", false, func(t *testing.T, r *productUpdateRun) {
			data := r.feed.files[r.feed.assetPath("Sailry-"+fixtureTarget+".zip")]
			data[len(data)-1] ^= 1
		}},
		{"trusted archive for another bundle identifier", "updates_package_invalid", false, func(t *testing.T, r *productUpdateRun) {
			other := r.feed.product.Release.Package
			other.ID = "test.fixture.not-code"
			r.feed.prepare(t, r.feed.private, other)
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			r := newProductUpdateRun(t, codeUpdateProductFixture(), fixtureTarget, true)
			test.mutate(t, r)
			r.service.Check()
			state, failure, notices := settle(r.service)
			if !test.checkFails {
				if state.Kind != Available || failure != nil || len(notices) != 0 {
					t.Fatalf("check = %+v, %v, %+v", state, failure, notices)
				}
				r.service.Download()
				state, failure, notices = settle(r.service)
			}
			want := Available
			if test.checkFails {
				want = Idle
			}
			if state.Kind != want || failure == nil || failure.Key != test.failure || len(notices) != 1 || !notices[0].Error || notices[0].Key != test.failure {
				t.Fatalf("refusal = %+v, %v, %+v", state, failure, notices)
			}
			r.service.Install()
			settle(r.service)
			if r.service.staged != nil || r.calls.stops != 0 || r.calls.exchanges != 0 || r.calls.quits != 0 || r.calls.executable != "" ||
				productUpdateBundleVersion(t, r.installed) != "0.1.0" {
				t.Fatalf("a refused 0.1.1 update changed the running application: %+v", r.calls)
			}
			if downloads, err := filepath.Glob(filepath.Join(r.root, "download-*")); err != nil || len(downloads) != 0 {
				t.Fatalf("refused download left staged contents: %v, %v", downloads, err)
			}
			if _, err := os.Stat(filepath.Join(r.root, "receipt.json")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("refused update wrote an install receipt: %v", err)
			}
			r.service.Close()
			r.assertDataUnchanged(t)
		})
	}
}

func TestCodeDesktopUpdateRevalidatesStaged011BeforeStopping(t *testing.T) {
	unsignedBundles(t)
	r := newProductUpdateRun(t, codeUpdateProductFixture(), fixtureTarget, true)
	checkProductUpdateAvailable(t, r)
	downloadProductUpdate(t, r)
	archive := r.service.staged.archive
	data, err := os.ReadFile(archive)
	if err != nil {
		t.Fatal(err)
	}
	data[len(data)-1] ^= 1
	if err := os.WriteFile(archive, data, 0o600); err != nil {
		t.Fatal(err)
	}
	r.service.Install()
	state, failure, notices := settle(r.service)
	if state.Kind != Ready || failure == nil || failure.Key != "updates_digest_invalid" || len(notices) != 1 || !notices[0].Error ||
		r.calls.stops != 0 || r.calls.exchanges != 0 || r.calls.quits != 0 || r.calls.executable != "" || productUpdateBundleVersion(t, r.installed) != "0.1.0" {
		t.Fatalf("changed staged archive was not refused before shutdown: %+v, %v, %+v, %+v", state, failure, notices, r.calls)
	}
	r.service.Close()
	r.assertDataUnchanged(t)
}

func checkProductUpdateAvailable(t *testing.T, r *productUpdateRun) {
	t.Helper()
	r.service.Check()
	state, failure, notices := settle(r.service)
	if state.Kind != Available || state.Version != "0.1.1" || failure != nil || len(notices) != 0 {
		t.Fatalf("0.1.0 check = %+v, %v, %+v", state, failure, notices)
	}
}

func downloadProductUpdate(t *testing.T, r *productUpdateRun) {
	t.Helper()
	r.service.Download()
	state, failure, notices := settle(r.service)
	if state.Kind != Ready || state.Version != "0.1.1" || failure != nil || len(notices) != 1 || notices[0].Key != "updates_ready" || notices[0].Version != "0.1.1" || notices[0].Error {
		t.Fatalf("0.1.1 download = %+v, %v, %+v", state, failure, notices)
	}
}
