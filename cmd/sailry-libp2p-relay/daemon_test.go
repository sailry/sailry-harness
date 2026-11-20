//go:build unix

package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"syscall"
	"testing"

	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
	"github.com/sailry/sailry-harness/internal/link"
)

func TestDaemonCLIValidatesArgumentsBeforePlatformAccess(t *testing.T) {
	for _, args := range [][]string{nil, {"uninstall"}, {"stop", "extra"}, {"install", "--config="}, {"install", "extra"}} {
		if err := runDaemon(t.Context(), args, io.Discard, io.Discard); err == nil {
			t.Fatalf("daemon accepted %v", args)
		}
	}
	var output bytes.Buffer
	if err := runDaemon(t.Context(), []string{"--help"}, &output, io.Discard); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "daemon install [--config <path>]") || !strings.Contains(output.String(), "Linux with systemd") {
		t.Fatalf("unexpected help: %s", &output)
	}
}

func TestDaemonInstallAndManage(t *testing.T) {
	f := newDaemonFixture(t)
	var output bytes.Buffer
	if err := f.daemon.manage(t.Context(), "install", []string{f.config}, &output); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "online and enabled at boot") || !slices.Equal(f.calls, []string{"show", "daemon-reload", "show", "start", "ready", "show", "enable"}) {
		t.Fatalf("install output %q, commands %v", output.String(), f.calls)
	}
	for _, pair := range [][2]string{{f.daemon.sourceBinary, f.daemon.binaryPath}, {f.config, f.daemon.configPath}} {
		if got, want := readDaemonTestFile(t, pair[1]), readDaemonTestFile(t, pair[0]); got != want {
			t.Fatalf("incorrect copy at %s", pair[1])
		}
	}
	unit := readDaemonTestFile(t, f.daemon.unitPath())
	for _, directive := range []string{"User=sailry-relay\n", "Group=sailry-relay\n", "Restart=always\n", "WantedBy=multi-user.target\n",
		fmt.Sprintf("ExecStart=%s start --config %s\n", f.daemon.binaryPath, f.daemon.configPath)} {
		if !strings.Contains(unit, directive) {
			t.Fatalf("unit does not contain %q", directive)
		}
	}
	id, err := readRelayPeerID(f.state)
	if err != nil {
		t.Fatal(err)
	}
	if err := f.daemon.manage(t.Context(), "stop", nil, io.Discard); err != nil {
		t.Fatal(err)
	}
	output.Reset()
	if err := f.daemon.manage(t.Context(), "status", nil, &output); err != nil {
		t.Fatal(err)
	}
	if got := output.String(); got != "sailry-relay.service: inactive (dead), enabled\n" {
		t.Fatalf("status after explicit stop = %q", got)
	}
	for _, action := range []string{"start", "restart"} {
		if err := f.daemon.manage(t.Context(), action, nil, io.Discard); err != nil {
			t.Fatalf("%s: %v", action, err)
		}
		if f.expected != id {
			t.Fatalf("%s probed PeerID %s, want %s", action, f.expected, id)
		}
	}
}

func TestDaemonFailedInstallPreservesIdentityAndExistingAssets(t *testing.T) {
	for _, existing := range []bool{false, true} {
		t.Run(fmt.Sprintf("existing=%t", existing), func(t *testing.T) {
			f := newDaemonFixture(t)
			failure := errors.New("injected install failure")
			var originalKey, originalConfig, originalBinary string
			if existing {
				f.accountExists = true
				if err := createRelayDaemonState(f.state, f.account); err != nil {
					t.Fatal(err)
				}
				if _, err := link.OpenIdentity(f.state); err != nil {
					t.Fatal(err)
				}
				originalKey = readDaemonTestFile(t, filepath.Join(f.state, "link.key"))
				originalConfig = readDaemonTestFile(t, f.config)
				originalBinary = readDaemonTestFile(t, f.daemon.sourceBinary)
				writeDaemonTestFile(t, f.daemon.configPath, originalConfig, 0o644)
				writeDaemonTestFile(t, f.daemon.binaryPath, originalBinary, 0o755)
				f.daemon.sourceBinary = f.daemon.binaryPath
				f.config = ""
				f.readyErr = failure
			} else {
				f.enableErr = failure
			}
			err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard)
			if !errors.Is(err, failure) || f.active || !slices.Contains(f.calls, "stop") {
				t.Fatalf("failed install: %v, active=%t, calls=%v", err, f.active, f.calls)
			}
			assertDaemonTestMissing(t, f.daemon.unitPath(), f.daemon.enablePath())
			key := readDaemonTestFile(t, filepath.Join(f.state, "link.key"))
			if existing {
				if key != originalKey || readDaemonTestFile(t, f.daemon.configPath) != originalConfig || readDaemonTestFile(t, f.daemon.binaryPath) != originalBinary {
					t.Fatal("rollback changed a pre-existing asset")
				}
			} else {
				assertDaemonTestMissing(t, f.daemon.binaryPath, f.daemon.configPath)
				if len(key) == 0 {
					t.Fatal("rollback did not retain the new identity")
				}
			}
		})
	}
}

func TestDaemonInstallRefusesOccupiedPort(t *testing.T) {
	f := newDaemonFixture(t)
	listener := listenDaemonTestPort(t)
	defer listener.Close()
	f.writeConfig(listener.Addr().(*net.TCPAddr).Port)
	err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard)
	if !errors.Is(err, syscall.EADDRINUSE) || !slices.Equal(f.calls, []string{"show"}) || f.accountExists {
		t.Fatalf("occupied port: %v, commands %v, account exists %t", err, f.calls, f.accountExists)
	}
	assertDaemonTestMissing(t, f.daemon.binaryPath, f.daemon.configPath, f.daemon.unitPath(), f.state)
}

func TestDaemonInstallRejectsPrivilegedPortBeforeCreatingAccount(t *testing.T) {
	f := newDaemonFixture(t)
	f.writeConfig(443)
	err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard)
	if err == nil || !strings.Contains(err.Error(), "cannot bind privileged TCP port 443") || f.accountExists || !slices.Equal(f.calls, []string{"show"}) {
		t.Fatalf("privileged port: %v, commands %v, account exists %t", err, f.calls, f.accountExists)
	}
	assertDaemonTestMissing(t, f.daemon.binaryPath, f.daemon.configPath, f.daemon.unitPath(), f.state)
}

func TestDaemonCheckPortWithoutUnprivilegedPortStart(t *testing.T) {
	d := relayDaemon{root: t.TempDir()}
	var options startOptions
	for _, port := range []int{4001, 1024} {
		options.config.ListenAddr = ma.StringCast(fmt.Sprintf("/ip4/127.0.0.1/tcp/%d", port))
		if err := d.checkPort(t.Context(), options, "active"); err != nil {
			t.Fatalf("port %d with missing sysctl: %v", port, err)
		}
	}
	options.config.ListenAddr = ma.StringCast("/ip4/127.0.0.1/tcp/1023")
	if err := d.checkPort(t.Context(), options, "active"); err == nil || !strings.Contains(err.Error(), "cannot bind privileged TCP port 1023") {
		t.Fatalf("port 1023 with missing sysctl: %v", err)
	}
}

func TestDaemonCheckPortPropagatesSysctlReadError(t *testing.T) {
	d := relayDaemon{root: t.TempDir()}
	if err := os.MkdirAll(filepath.Join(d.root, "proc/sys/net/ipv4/ip_unprivileged_port_start"), 0o755); err != nil {
		t.Fatal(err)
	}
	var options startOptions
	options.config.ListenAddr = ma.StringCast("/ip4/127.0.0.1/tcp/4001")
	if err := d.checkPort(t.Context(), options, "active"); !errors.Is(err, syscall.EISDIR) {
		t.Fatalf("unreadable sysctl: got %v, want EISDIR", err)
	}
}

func TestDaemonActiveRestartHonorsUnprivilegedPortStart(t *testing.T) {
	f := newDaemonFixture(t)
	if err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard); err != nil {
		t.Fatal(err)
	}
	f.config = f.daemon.configPath
	f.writeConfig(443)
	for _, test := range []struct{ first, wantErr string }{
		{"0", ""}, {"443", ""}, {"444", "cannot bind privileged TCP port 443"}, {"invalid", "invalid proc/sys"},
	} {
		f.calls = nil
		writeDaemonTestFile(t, filepath.Join(f.daemon.root, "proc/sys/net/ipv4/ip_unprivileged_port_start"), test.first+"\n", 0o644)
		err := f.daemon.manage(t.Context(), "restart", nil, io.Discard)
		if test.wantErr != "" {
			if err == nil || !strings.Contains(err.Error(), test.wantErr) || !slices.Equal(f.calls, []string{"show"}) {
				t.Fatalf("first port %s: %v, commands %v", test.first, err, f.calls)
			}
		} else if err != nil || !slices.Contains(f.calls, "restart") {
			t.Fatalf("first port %s: %v, commands %v", test.first, err, f.calls)
		}
	}
}

func TestDaemonInstallRejectsWrongSystemdExecutable(t *testing.T) {
	f := newDaemonFixture(t)
	path := f.processExecutable()
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(f.daemon.sourceBinary, path); err != nil {
		t.Fatal(err)
	}
	err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard)
	if err == nil || !strings.Contains(err.Error(), "not running the installed relay executable") || f.active || slices.Contains(f.calls, "enable") {
		t.Fatalf("wrong executable: %v, commands %v", err, f.calls)
	}
	assertDaemonTestMissing(t, f.daemon.unitPath())
	if _, err := readRelayPeerID(f.state); err != nil {
		t.Fatal(err)
	}
}

func TestDaemonInstallRefusesUnknownStateAndFiles(t *testing.T) {
	for _, kind := range []string{"state symlink", "state wrong owner", "state unknown file", "foreign executable", "foreign config"} {
		t.Run(kind, func(t *testing.T) {
			f := newDaemonFixture(t)
			var path string
			switch kind {
			case "state symlink":
				path = f.state
				if err := os.Symlink(t.TempDir(), path); err != nil {
					t.Fatal(err)
				}
			case "state wrong owner":
				path = f.state
				if err := os.Mkdir(path, 0o700); err != nil {
					t.Fatal(err)
				}
			case "state unknown file":
				f.accountExists = true
				if err := createRelayDaemonState(f.state, f.account); err != nil {
					t.Fatal(err)
				}
				path = filepath.Join(f.state, "other.key")
				writeDaemonTestFile(t, path, "unknown state", 0o600)
			case "foreign executable":
				path = f.daemon.binaryPath
				writeDaemonTestFile(t, path, "foreign executable", 0o755)
			case "foreign config":
				path = f.daemon.configPath
				writeDaemonTestFile(t, path, "foreign config", 0o644)
			}
			before, err := os.Lstat(path)
			if err != nil {
				t.Fatal(err)
			}
			if err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard); err == nil || !slices.Equal(f.calls, []string{"show"}) {
				t.Fatalf("install accepted %s: %v, commands %v", kind, err, f.calls)
			}
			after, err := os.Lstat(path)
			if err != nil || !os.SameFile(before, after) {
				t.Fatalf("install replaced %s: %v", kind, err)
			}
			assertDaemonTestMissing(t, f.daemon.unitPath(), f.daemon.enablePath())
		})
	}
}

func TestDaemonUpdateRejectsMissingManagedIdentity(t *testing.T) {
	f := newDaemonFixture(t)
	if err := f.daemon.manage(t.Context(), "install", []string{f.config}, io.Discard); err != nil {
		t.Fatal(err)
	}
	key := filepath.Join(f.state, "link.key")
	if err := os.Remove(key); err != nil {
		t.Fatal(err)
	}
	f.calls = nil
	args := []string{f.daemon.sourceBinary, "--sha256", strings.Repeat("0", 64)}
	err := f.daemon.manage(t.Context(), "update", args, io.Discard)
	if err == nil || !strings.Contains(err.Error(), "managed relay identity is missing") || !slices.Equal(f.calls, []string{"show"}) {
		t.Fatalf("update with missing identity: %v, commands %v", err, f.calls)
	}
	assertDaemonTestMissing(t, key, f.daemon.binaryPath+".previous")
}

func TestDaemonRefusesForeignEffectiveUnit(t *testing.T) {
	f := newDaemonFixture(t)
	writeDaemonTestFile(t, f.daemon.unitPath(), f.daemon.unitContents(), 0o644)
	for _, test := range []struct{ fragment, dropIn string }{
		{"/usr/lib/systemd/system/sailry-relay.service", ""},
		{f.daemon.unitPath(), "/etc/systemd/system/sailry-relay.service.d/override.conf"},
	} {
		f.fragment, f.dropIn = test.fragment, test.dropIn
		f.calls = nil
		for _, action := range []string{"start", "stop", "restart", "status", "update"} {
			if err := f.daemon.manage(t.Context(), action, nil, io.Discard); err == nil {
				t.Fatalf("%s accepted foreign unit %+v", action, test)
			}
		}
		if !slices.Equal(f.calls, []string{"show", "show", "show", "show", "show"}) {
			t.Fatalf("foreign unit received commands: %v", f.calls)
		}
	}
}

type daemonFixture struct {
	t                     *testing.T
	daemon                relayDaemon
	config, state         string
	account               relayDaemonAccount
	locked, accountExists bool
	active, enabled       bool
	fragment, dropIn      string
	calls                 []string
	expected              peer.ID
	readyErr, enableErr   error
}

const daemonTestPID = 123

func newDaemonFixture(t *testing.T) *daemonFixture {
	t.Helper()
	root := t.TempDir()
	f := &daemonFixture{
		t:       t,
		config:  filepath.Join(root, "source.toml"),
		state:   filepath.Join(root, "var/lib/relay"),
		account: relayDaemonAccount{uid: os.Geteuid(), gid: os.Getegid()},
	}
	f.daemon = relayDaemon{
		root:         root,
		adminUID:     os.Geteuid(),
		configPath:   filepath.Join(root, "etc/sailry-relay", relayConfigName),
		binaryPath:   filepath.Join(root, "usr/local/bin/sailry-relay"),
		sourceBinary: filepath.Join(root, "download/sailry-relay"),
		ctl:          f.control,
		fileIDs: func(info os.FileInfo) (int, int, uint64) {
			stat := info.Sys().(*syscall.Stat_t)
			return int(stat.Uid), int(stat.Gid), uint64(stat.Nlink)
		},
		lock: func(_ string, _ int) (io.Closer, error) {
			if f.locked {
				t.Fatal("lock already held")
			}
			f.locked = true
			return daemonTestClose(func() error { f.locked = false; return nil }), nil
		},
		account: func(_ context.Context, create bool) (relayDaemonAccount, error) {
			if !f.locked {
				t.Fatal("account lookup without management lock")
			}
			if create {
				f.accountExists = true
			}
			if !f.accountExists {
				return relayDaemonAccount{}, errRelayDaemonAccountMissing
			}
			return f.account, nil
		},
		ready: func(_ context.Context, opts startOptions, expected peer.ID) error {
			if !f.locked || opts.directory != f.state || !f.active {
				t.Fatalf("unexpected readiness probe: dir=%s active=%t locked=%t", opts.directory, f.active, f.locked)
			}
			f.calls = append(f.calls, "ready")
			f.expected = expected
			return f.readyErr
		},
	}
	for _, path := range []string{"etc/systemd/system", "usr/local", "var/lib", "run"} {
		if err := os.MkdirAll(filepath.Join(root, path), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.MkdirAll(filepath.Dir(f.processExecutable()), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(f.daemon.binaryPath, f.processExecutable()); err != nil {
		t.Fatal(err)
	}
	writeDaemonTestFile(t, filepath.Join(root, "proc/sys/net/ipv4/ip_unprivileged_port_start"), "1024\n", 0o644)
	writeDaemonTestFile(t, f.daemon.sourceBinary, "relay executable", 0o755)
	listener := listenDaemonTestPort(t)
	port := listener.Addr().(*net.TCPAddr).Port
	if err := listener.Close(); err != nil {
		t.Fatal(err)
	}
	f.writeConfig(port)
	return f
}

func (f *daemonFixture) processExecutable() string {
	return filepath.Join(f.daemon.root, "proc", fmt.Sprint(daemonTestPID), "exe")
}

func (f *daemonFixture) writeConfig(port int) {
	writeDaemonTestFile(f.t, f.config, fmt.Sprintf("mode = 'public'\nport = %d\ndata_dir = %q\n", port, f.state), 0o644)
}

func (f *daemonFixture) control(ctx context.Context, args ...string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	action := args[0]
	f.calls = append(f.calls, action)
	if action != "show" && !f.locked {
		f.t.Fatalf("systemctl %s without management lock", action)
	}
	switch action {
	case "show":
		active, sub, enabled := "inactive", "dead", "disabled"
		pid := 0
		if f.active {
			active, sub = "active", "running"
			pid = daemonTestPID
		}
		if f.enabled {
			enabled = "enabled"
		}
		fragment := f.fragment
		if fragment == "" {
			if _, err := os.Lstat(f.daemon.unitPath()); errors.Is(err, os.ErrNotExist) {
				return "LoadState=not-found\nFragmentPath=\nDropInPaths=\nActiveState=inactive", errors.New("unit not found")
			} else if err != nil {
				return "", err
			}
			fragment = f.daemon.unitPath()
		}
		return fmt.Sprintf("LoadState=loaded\nFragmentPath=%s\nDropInPaths=%s\nNeedDaemonReload=no\nActiveState=%s\nSubState=%s\nMainPID=%d\nUnitFileState=%s", fragment, f.dropIn, active, sub, pid, enabled), nil
	case "start", "restart":
		if _, err := link.OpenIdentity(f.state); err != nil {
			return "", err
		}
		f.active = true
	case "stop":
		f.active = false
	case "enable":
		path := f.daemon.enablePath()
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return "", err
		}
		if err := os.Symlink(f.daemon.unitPath(), path); err != nil {
			return "", err
		}
		if f.enableErr != nil {
			return "", f.enableErr
		}
		f.enabled = true
	case "daemon-reload":
	default:
		f.t.Fatalf("unexpected systemctl arguments %v", args)
	}
	return "", nil
}

type daemonTestClose func() error

func (closeLock daemonTestClose) Close() error { return closeLock() }

func listenDaemonTestPort(t *testing.T) net.Listener {
	t.Helper()
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	return listener
}

func writeDaemonTestFile(t *testing.T, path, contents string, mode os.FileMode) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(contents), mode); err != nil {
		t.Fatal(err)
	}
}

func readDaemonTestFile(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(data)
}

func assertDaemonTestMissing(t *testing.T, paths ...string) {
	t.Helper()
	for _, path := range paths {
		if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("expected %s to be absent: %v", path, err)
		}
	}
}
