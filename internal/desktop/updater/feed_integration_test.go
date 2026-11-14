//go:build integration

package updater

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// TestUpdaterLocalFeed installs a release from a local feed signed with a
// generated ed25519 test key into a fixture bundle. The bundle is unsigned
// and no process is started, so it does not prove a real signed update.
func TestUpdaterLocalFeed(t *testing.T) {
	unsignedBundles(t)
	type handoff struct {
		stops, quits int
		launched     []string
		stopErr      error
	}
	start := func(t *testing.T, current string) (*Service, *handoff, string, string) {
		t.Helper()
		source := newFeed(t)
		source.publish(fixtureRelease{version: "1.1.0"})
		home := t.TempDir()
		installed := filepath.Join(home, "Applications", "Sailry.app")
		for name, data := range bundleFiles(fixtureTarget, current, "12.0") {
			path := filepath.Join(installed, filepath.FromSlash(name))
			if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, data, 0o755); err != nil {
				t.Fatal(err)
			}
		}
		profile := filepath.Join(home, "profile")
		if err := os.MkdirAll(profile, 0o700); err != nil {
			t.Fatal(err)
		}
		calls := &handoff{}
		root := filepath.Join(home, "preferences", "updater")
		service := New(Options{Version: current, Directory: root, Profile: profile, Channel: func() Channel { return Stable },
			Stop: func() error { calls.stops++; return calls.stopErr }, Quit: func() { calls.quits++ }})
		service.release = func(channel Channel, version string) (Config, error) { return source.config(channel, version), nil }
		service.installed = func() (string, error) { return installed, nil }
		service.relaunch = func(executable string, _ []string) error {
			calls.launched = append(calls.launched, executable)
			return nil
		}
		t.Cleanup(service.Close)
		service.Check()
		settle(service)
		service.Download()
		if state, failure, notices := settle(service); state.Kind != Ready || failure != nil || len(notices) != 1 || notices[0].Version != "1.1.0" {
			t.Fatalf("download = %+v %v %+v", state, failure, notices)
		}
		return service, calls, installed, root
	}
	packaged := func(t *testing.T, bundle string) string {
		t.Helper()
		data, err := os.ReadFile(filepath.Join(bundle, "Contents", "Resources", "build.json"))
		if err != nil {
			t.Fatal(err)
		}
		var value build
		if err := json.Unmarshal(data, &value); err != nil {
			t.Fatal(err)
		}
		return value.ApplicationVersion
	}

	t.Run("install, restart and acknowledge", func(t *testing.T) {
		service, calls, installed, root := start(t, "1.0.0")
		service.Install()
		if state, _ := service.Snapshot(); state.Kind != Installing {
			t.Fatalf("install started in %+v", state)
		}
		if _, failure, notices := settle(service); failure != nil || len(notices) != 0 {
			t.Fatalf("install = %v %+v", failure, notices)
		}
		if calls.stops != 1 || calls.quits != 1 || len(calls.launched) != 1 || calls.launched[0] != filepath.Join(installed, "Contents", "MacOS", "sailry-desktop") {
			t.Fatalf("handoff = %+v", calls)
		}
		if version := packaged(t, installed); version != "1.1.0" {
			t.Fatalf("installed version = %s", version)
		}
		if runtime.GOOS == "windows" {
			metadata, err := os.ReadFile(filepath.Join(installed, "Contents", "Resources", "build.json"))
			if err != nil {
				t.Fatal(err)
			}
			current, err := os.ReadFile(filepath.Join(installed, "Contents", "Resources", "current.json"))
			if err != nil || string(current) != string(metadata) {
				t.Fatalf("metadata resource = %q %v", current, err)
			}
		} else {
			if target, err := os.Readlink(filepath.Join(installed, "Contents", "Resources", "current.json")); err != nil || target != "build.json" {
				t.Fatalf("symbolic link = %q %v", target, err)
			}
			if info, err := os.Stat(filepath.Join(installed, "Contents", "MacOS", "sailry-desktop")); err != nil || info.Mode().Perm()&0o100 == 0 {
				t.Fatalf("executable mode = %v %v", info, err)
			}
		}
		backups, _ := filepath.Glob(filepath.Join(filepath.Dir(installed), ".sailry-update-backup-*"))
		if len(backups) != 1 || packaged(t, filepath.Join(backups[0], "Sailry.app")) != "1.0.0" {
			t.Fatalf("backups = %v", backups)
		}
		if leftovers, _ := filepath.Glob(filepath.Join(filepath.Dir(installed), ".sailry-update-*")); len(leftovers) != 1 {
			t.Fatalf("work directories remain: %v", leftovers)
		}

		// The next start of the new version acknowledges the receipt once
		// and removes the retained original.
		next := New(Options{Version: "1.1.0", Directory: root})
		next.installed = func() (string, error) { return installed, nil }
		next.Startup()
		if _, failure, notices := settle(next); failure != nil || len(notices) != 1 || notices[0].Key != "updates_completed" || notices[0].Error {
			t.Fatalf("startup = %v %+v", failure, notices)
		}
		if _, err := os.Stat(backups[0]); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("backup remains: %v", err)
		}
		archived, _ := filepath.Glob(filepath.Join(root, "receipts", "receipt-*.json"))
		if _, err := os.Stat(filepath.Join(root, "receipt.json")); !errors.Is(err, os.ErrNotExist) || len(archived) != 1 {
			t.Fatalf("receipt was not archived: %v %v", err, archived)
		}
		next.Startup()
		if _, _, notices := settle(next); len(notices) != 0 {
			t.Fatalf("receipt was reported twice: %+v", notices)
		}
	})

	t.Run("a restart of the old version is a failed restart", func(t *testing.T) {
		service, _, installed, root := start(t, "1.0.0")
		service.Install()
		settle(service)
		next := New(Options{Version: "1.0.0", Directory: root})
		next.installed = func() (string, error) { return installed, nil }
		next.Startup()
		if _, failure, notices := settle(next); failure == nil || failure.Key != "updates_restart_failed" || len(notices) != 1 || !notices[0].Error {
			t.Fatalf("startup = %v %+v", failure, notices)
		}
		if backups, _ := filepath.Glob(filepath.Join(filepath.Dir(installed), ".sailry-update-backup-*")); len(backups) != 1 {
			t.Fatalf("the original was not retained: %v", backups)
		}
	})

	t.Run("a Node that cannot stop leaves only restart", func(t *testing.T) {
		service, calls, installed, _ := start(t, "1.0.0")
		calls.stopErr = errors.New("local Node is busy")
		service.Install()
		state, failure, notices := settle(service)
		if state.Kind != Stopped || failure == nil || failure.Key != "updates_shutdown_failed" || len(notices) != 1 || calls.quits != 0 || packaged(t, installed) != "1.0.0" {
			t.Fatalf("install = %+v %v %+v", state, failure, notices)
		}
		service.Check()
		service.Download()
		service.Install()
		if state, _, _ := settle(service); state.Kind != Stopped {
			t.Fatalf("a stopped service moved to %+v", state)
		}
		calls.stopErr = nil
		service.Restart()
		if _, _, notices := settle(service); calls.quits != 1 || len(calls.launched) != 1 || len(notices) != 0 {
			t.Fatalf("restart = %+v %+v", calls, notices)
		}
	})

	t.Run("an unacknowledged receipt blocks installation", func(t *testing.T) {
		service, calls, installed, root := start(t, "1.0.0")
		if err := os.WriteFile(filepath.Join(root, "receipt.json"), []byte(`{"version":1,"result":"failure","uncertain":true,"message":"kept"}`), 0o600); err != nil {
			t.Fatal(err)
		}
		service.Install()
		state, failure, _ := settle(service)
		if state.Kind != Ready || failure == nil || failure.Key != "updates_receipt_unread" || calls.stops != 0 || packaged(t, installed) != "1.0.0" {
			t.Fatalf("install = %+v %v %+v", state, failure, calls)
		}
		service.Startup()
		if _, failure, _ := settle(service); failure == nil || failure.Key != "updates_install_uncertain" || failure.Detail != "kept" {
			t.Fatalf("startup = %v", failure)
		}
	})

	t.Run("a profile inside the bundle is never replaced", func(t *testing.T) {
		service, calls, installed, _ := start(t, "1.0.0")
		inside := filepath.Join(installed, "Contents", "profile")
		if err := os.MkdirAll(inside, 0o700); err != nil {
			t.Fatal(err)
		}
		service.options.Profile = inside
		service.Install()
		state, failure, _ := settle(service)
		if state.Kind != Ready || failure == nil || failure.Key != "updates_profile_inside" || calls.stops != 0 {
			t.Fatalf("install = %+v %v", state, failure)
		}
	})

	t.Run("a staged archive changed after download is refused", func(t *testing.T) {
		service, calls, installed, _ := start(t, "1.0.0")
		archive := service.staged.archive
		data, err := os.ReadFile(archive)
		if err != nil {
			t.Fatal(err)
		}
		data[len(data)-1] ^= 1
		if err := os.WriteFile(archive, data, 0o600); err != nil {
			t.Fatal(err)
		}
		service.Install()
		state, failure, _ := settle(service)
		if state.Kind != Ready || failure == nil || failure.Key != "updates_digest_invalid" || calls.stops != 0 || packaged(t, installed) != "1.0.0" {
			t.Fatalf("install = %+v %v", state, failure)
		}
	})

	t.Run("an unbundled build cannot install", func(t *testing.T) {
		service, _, _, _ := start(t, "1.0.0")
		service.installed = func() (string, error) {
			return "", failure("updates_bundle_required", "updates require a packaged macOS application")
		}
		service.Install()
		if state, failure, _ := settle(service); state.Kind != Ready || failure == nil || !strings.HasPrefix(failure.Key, "updates_bundle_required") {
			t.Fatalf("install = %+v %v", state, failure)
		}
	})
}
