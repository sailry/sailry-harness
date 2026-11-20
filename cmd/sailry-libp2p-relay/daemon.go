package main

import (
	"bytes"
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
)

const relayDaemonUsage = `Usage (Linux with systemd):
  sailry-relay daemon install [--config <path>]
  sailry-relay daemon start|stop|restart|status
  sailry-relay daemon update [<binary> --sha256 <digest>]

Install uses /etc/sailry-relay/relay.toml if present, otherwise ./relay.toml.
Install starts the service and enables it at boot. Only status can run without root.
Use sailry-relay start for a foreground relay managed by another supervisor.`

const (
	relayDaemonUser = "sailry-relay"
	relayDaemonUnit = "sailry-relay.service"
)

type relayDaemon struct {
	binaryPath string
	configPath string
	ctl        func(context.Context, ...string) (string, error)

	root         string
	adminUID     int
	sourceBinary string
	fileIDs      func(os.FileInfo) (uid, gid int, links uint64)
	lock         func(string, int) (io.Closer, error)
	account      func(context.Context, bool) (relayDaemonAccount, error)
	ready        func(context.Context, startOptions, peer.ID) error
}

type relayDaemonAccount struct{ uid, gid int }

var errRelayDaemonAccountMissing = errors.New("the dedicated sailry-relay account does not exist")

func runDaemon(ctx context.Context, args []string, output, diagnostics io.Writer) error {
	if len(args) == 0 {
		return errors.New("expected a daemon command; use daemon --help")
	}
	action, rest := args[0], args[1:]
	switch action {
	case "-h", "--help", "help":
		_, err := fmt.Fprintln(output, relayDaemonUsage)
		return err
	case "install", "update", "start", "stop", "restart", "status":
	default:
		return fmt.Errorf("unknown daemon command %q; use daemon --help", action)
	}
	if len(rest) == 1 && (rest[0] == "-h" || rest[0] == "--help") {
		_, err := fmt.Fprintln(output, relayDaemonUsage)
		return err
	}
	switch action {
	case "install":
		flags := flag.NewFlagSet("sailry-relay daemon install", flag.ContinueOnError)
		flags.SetOutput(diagnostics)
		source := flags.String("config", "", "Path to the relay TOML configuration")
		if err := flags.Parse(rest); err != nil {
			if errors.Is(err, flag.ErrHelp) {
				return nil
			}
			return err
		}
		if flags.NArg() != 0 || (flags.NFlag() != 0 && *source == "") {
			return errors.New("daemon install accepts only an optional non-empty --config path")
		}
		rest = []string{*source}
	case "update":
	default:
		if len(rest) != 0 {
			return fmt.Errorf("daemon %s does not accept arguments", action)
		}
	}
	return runPlatformDaemon(ctx, action, rest, output)
}

func (d *relayDaemon) unitPath() string {
	return filepath.Join(d.root, "etc/systemd/system", relayDaemonUnit)
}

func (d *relayDaemon) enablePath() string {
	return filepath.Join(filepath.Dir(d.unitPath()), "multi-user.target.wants", relayDaemonUnit)
}

func (d *relayDaemon) lockPath() string { return filepath.Join(d.root, "run/sailry-relay.lock") }

func (d *relayDaemon) manage(ctx context.Context, action string, args []string, output io.Writer) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if action != "status" {
		lock, err := d.openManagementLock()
		if err != nil {
			return err
		}
		defer lock.Close()
	}
	if action == "install" {
		return d.install(ctx, args[0], output)
	}
	properties, err := d.ownedUnit(ctx)
	if err != nil {
		return err
	}
	switch action {
	case "status":
		_, err := fmt.Fprintf(output, "%s: %s (%s), %s\n", relayDaemonUnit, properties["ActiveState"], properties["SubState"], properties["UnitFileState"])
		return err
	case "stop":
		_, err := d.ctl(ctx, "stop", relayDaemonUnit)
		return err
	}
	if err := d.checkInstalledFile(d.binaryPath, 0o001); err != nil {
		return err
	}
	if err := d.checkInstalledFile(d.configPath, 0o004); err != nil {
		return err
	}
	options, _, err := readRelayConfig(d.configPath)
	if err != nil {
		return err
	}
	account, err := d.account(ctx, false)
	if err != nil {
		return err
	}
	_, id, err := d.inspectState(options.directory, account)
	if err != nil {
		return err
	}
	if id == "" {
		return errors.New("managed relay identity is missing; refusing to generate a replacement")
	}
	if action == "update" {
		return d.update(ctx, args, output)
	}
	switch state := properties["ActiveState"]; state {
	case "active":
	case "inactive", "failed":
		if err := checkRelayDaemonPort(ctx, options); err != nil {
			return err
		}
	default:
		return fmt.Errorf("relay service is %s; wait for it to settle before %s", state, action)
	}
	return d.activate(ctx, action, options, id)
}

func (d *relayDaemon) install(ctx context.Context, configSource string, output io.Writer) (result error) {
	if err := d.absent(d.unitPath()); err != nil {
		return err
	}
	properties, err := d.unitProperties(ctx)
	if err != nil {
		return err
	}
	if properties["LoadState"] != "not-found" || properties["ActiveState"] != "inactive" || properties["FragmentPath"] != "" || properties["DropInPaths"] != "" {
		return fmt.Errorf("systemd already knows %s; refusing to take over another service", relayDaemonUnit)
	}
	if err := d.absentEnableLink(); err != nil {
		return err
	}
	configDir, binaryDir := filepath.Dir(d.configPath), filepath.Dir(d.binaryPath)
	configDirExists, err := d.adminDirectory(configDir)
	if err != nil {
		return err
	}
	binaryDirExists, err := d.adminDirectory(binaryDir)
	if err != nil {
		return err
	}
	configExists, err := d.optionalAdminFile(d.configPath, 0o004)
	if err != nil {
		return err
	}
	if configSource == "" {
		configSource = relayConfigName
		if configExists != nil {
			configSource = d.configPath
		}
	}
	configSource, err = filepath.Abs(configSource)
	if err != nil {
		return err
	}
	if configExists != nil && configSource != d.configPath {
		return fmt.Errorf("refusing to replace existing configuration %s", d.configPath)
	}
	if info, err := os.Lstat(configSource); err != nil {
		return err
	} else if !info.Mode().IsRegular() {
		return fmt.Errorf("relay config %s must be a regular file", configSource)
	}
	options, config, err := readRelayConfig(configSource)
	if err != nil {
		return err
	}
	source, err := os.Open(d.sourceBinary)
	if err != nil {
		return fmt.Errorf("open running relay executable: %w", err)
	}
	defer source.Close()
	binaryExists, err := d.optionalAdminFile(d.binaryPath, 0o001)
	if err != nil {
		return err
	}
	if binaryExists != nil {
		info, err := source.Stat()
		if err != nil {
			return err
		}
		if !os.SameFile(binaryExists, info) {
			return fmt.Errorf("refusing to replace existing executable %s", d.binaryPath)
		}
	}
	if err := checkRelayDaemonPort(ctx, options); err != nil {
		return err
	}
	account, err := d.account(ctx, false)
	if errors.Is(err, errRelayDaemonAccountMissing) {
		account = relayDaemonAccount{uid: -1, gid: -1}
	} else if err != nil {
		return err
	}
	stateExists, id, err := d.inspectState(options.directory, account)
	if err != nil {
		return err
	}
	if account.uid < 0 {
		account, err = d.account(ctx, true)
		if err != nil {
			return err
		}
	}

	var files relayDaemonNewFiles
	started, enabling, reload, complete := false, false, false, false
	defer func() {
		if complete {
			return
		}
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 30*time.Second)
		defer cancel()
		if started {
			if _, err := d.ownedUnit(cleanup); err != nil {
				result = errors.Join(result, fmt.Errorf("could not verify the service after install failure; keeping installation files: %w", err))
				return
			}
			if _, err := d.ctl(cleanup, "stop", relayDaemonUnit); err != nil {
				result = errors.Join(result, fmt.Errorf("could not stop the service after install failure; keeping installation files: %w", err))
				return
			}
		}
		var cleanupErr error
		if enabling {
			cleanupErr = d.removeEnableLink()
		}
		cleanupErr = errors.Join(cleanupErr, files.remove())
		if reload {
			_, err := d.ctl(cleanup, "daemon-reload")
			cleanupErr = errors.Join(cleanupErr, err)
		}
		if cleanupErr != nil {
			result = errors.Join(result, fmt.Errorf("clean up failed relay installation: %w", cleanupErr))
		}
	}()
	if !binaryDirExists {
		if err := files.mkdir(binaryDir); err != nil {
			return err
		}
	}
	if !configDirExists {
		if err := files.mkdir(configDir); err != nil {
			return err
		}
	}
	if binaryExists == nil {
		if err := files.write(d.binaryPath, 0o755, source); err != nil {
			return err
		}
	}
	if configExists == nil {
		if err := files.write(d.configPath, 0o644, bytes.NewReader(config)); err != nil {
			return err
		}
	}
	if !stateExists {
		if err := createRelayDaemonState(options.directory, account); err != nil {
			return err
		}
	}
	if err := files.write(d.unitPath(), 0o644, strings.NewReader(d.unitContents())); err != nil {
		return err
	}
	reload = true
	if _, err := d.ctl(ctx, "daemon-reload"); err != nil {
		return err
	}
	if _, err := d.ownedUnit(ctx); err != nil {
		return err
	}
	started = true
	if err := d.activate(ctx, "start", options, id); err != nil {
		return err
	}
	if err := d.absentEnableLink(); err != nil {
		return err
	}
	enabling = true
	if _, err := d.ctl(ctx, "enable", relayDaemonUnit); err != nil {
		return err
	}
	complete = true
	_, err = fmt.Fprintf(output, "Installed %s; the relay is online and enabled at boot.\n", relayDaemonUnit)
	return err
}

func (d *relayDaemon) activate(ctx context.Context, action string, options startOptions, expected peer.ID) error {
	if _, err := d.ctl(ctx, action, relayDaemonUnit); err != nil {
		return err
	}
	if err := d.ready(ctx, options, expected); err != nil {
		return err
	}
	return d.verifyRunningUnit(ctx)
}

func (d *relayDaemon) verifyRunningUnit(ctx context.Context) error {
	properties, err := d.ownedUnit(ctx)
	if err != nil {
		return err
	}
	if properties["ActiveState"] != "active" || properties["SubState"] != "running" {
		return fmt.Errorf("%s is not running under systemd", relayDaemonUnit)
	}
	pid, err := strconv.Atoi(properties["MainPID"])
	if err != nil || pid <= 0 {
		return fmt.Errorf("%s has no main process", relayDaemonUnit)
	}
	binary, err := d.adminFile(d.binaryPath, 0o001)
	if err != nil {
		return err
	}
	process, err := os.Stat(filepath.Join(d.root, "proc", strconv.Itoa(pid), "exe"))
	if err != nil {
		return fmt.Errorf("check the executable for relay process %d: %w", pid, err)
	}
	if !os.SameFile(process, binary) {
		return fmt.Errorf("%s is not running the installed relay executable", relayDaemonUnit)
	}
	return nil
}

func (d *relayDaemon) unitContents() string {
	return fmt.Sprintf(`# Managed by sailry-relay. Use sailry-relay daemon to manage this service.
[Unit]
Description=Sailry libp2p relay
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=%s
Group=%s
ExecStart=%s start --config %s
Restart=always
RestartSec=5s
NoNewPrivileges=true
UMask=0077

[Install]
WantedBy=multi-user.target
`, relayDaemonUser, relayDaemonUser, d.binaryPath, d.configPath)
}

func (d *relayDaemon) ownedUnit(ctx context.Context) (map[string]string, error) {
	path := d.unitPath()
	if err := d.trustedParents(path); err != nil {
		return nil, err
	}
	info, err := d.adminFile(path, 0o004)
	if err != nil {
		return nil, fmt.Errorf("check managed unit: %w", err)
	}
	contents := d.unitContents()
	if info.Size() != int64(len(contents)) {
		return nil, fmt.Errorf("%s is not a managed relay unit", path)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if string(data) != contents {
		return nil, fmt.Errorf("%s is not a managed relay unit", path)
	}
	properties, err := d.unitProperties(ctx)
	if err != nil {
		return nil, err
	}
	dropIns, checkedDropIns := properties["DropInPaths"]
	if properties["LoadState"] != "loaded" || properties["FragmentPath"] != path || !checkedDropIns || dropIns != "" || properties["NeedDaemonReload"] != "no" {
		return nil, fmt.Errorf("systemd has not loaded %s unchanged from %s; refusing to manage it", relayDaemonUnit, path)
	}
	return properties, nil
}

func (d *relayDaemon) unitProperties(ctx context.Context) (map[string]string, error) {
	text, err := d.ctl(ctx, "show", "--all", "--property=LoadState", "--property=FragmentPath", "--property=DropInPaths",
		"--property=NeedDaemonReload", "--property=ActiveState", "--property=SubState", "--property=MainPID", "--property=UnitFileState", relayDaemonUnit)
	properties := make(map[string]string)
	for line := range strings.SplitSeq(text, "\n") {
		if key, value, ok := strings.Cut(line, "="); ok {
			properties[key] = value
		}
	}
	if err != nil && properties["LoadState"] != "not-found" {
		return nil, err
	}
	if properties["LoadState"] == "" || properties["ActiveState"] == "" {
		return nil, errors.New("systemd did not report relay unit state")
	}
	return properties, nil
}

func (d *relayDaemon) trustedParents(path string) error {
	if !filepath.IsAbs(path) || path == d.root {
		return fmt.Errorf("unsafe managed relay path %s", path)
	}
	for parent := filepath.Dir(path); ; parent = filepath.Dir(parent) {
		if err := d.checkAdminDirectory(parent); err != nil {
			return err
		}
		if parent == d.root {
			return nil
		}
		if parent == filepath.Dir(parent) {
			return fmt.Errorf("managed path %s is outside %s", path, d.root)
		}
	}
}

func (d *relayDaemon) adminDirectory(path string) (bool, error) {
	if err := d.trustedParents(path); err != nil {
		return false, err
	}
	err := d.checkAdminDirectory(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	return err == nil, err
}

func (d *relayDaemon) checkAdminDirectory(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	uid, _, _ := d.fileIDs(info)
	if !info.IsDir() || info.Mode().Perm()&0o022 != 0 || uid != d.adminUID {
		return fmt.Errorf("unsafe relay directory %s; expected administrator ownership, no symlinks, and no group/other writes", path)
	}
	return nil
}

func (d *relayDaemon) adminFile(path string, access os.FileMode) (os.FileInfo, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0o022 != 0 || info.Mode().Perm()&access != access || info.Mode()&(os.ModeSetuid|os.ModeSetgid) != 0 {
		return nil, fmt.Errorf("refusing unsafe managed file %s", path)
	}
	uid, _, links := d.fileIDs(info)
	if uid != d.adminUID || links != 1 {
		return nil, fmt.Errorf("refusing unsafe ownership or hard links for %s", path)
	}
	return info, nil
}

func (d *relayDaemon) checkInstalledFile(path string, access os.FileMode) error {
	if err := d.trustedParents(path); err != nil {
		return err
	}
	_, err := d.adminFile(path, access)
	return err
}

func (d *relayDaemon) optionalAdminFile(path string, access os.FileMode) (os.FileInfo, error) {
	info, err := d.adminFile(path, access)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	return info, err
}

func (d *relayDaemon) absent(path string) error {
	if err := d.trustedParents(path); err != nil {
		return err
	}
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	return fmt.Errorf("refusing to replace %s; use start or update for an existing managed service", path)
}

func (d *relayDaemon) absentEnableLink() error {
	path := d.enablePath()
	exists, err := d.adminDirectory(filepath.Dir(path))
	if err != nil || !exists {
		return err
	}
	return d.absent(path)
}

func (d *relayDaemon) removeEnableLink() error {
	path := d.enablePath()
	exists, err := d.adminDirectory(filepath.Dir(path))
	if err != nil || !exists {
		return err
	}
	info, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	uid, _, _ := d.fileIDs(info)
	if info.Mode()&os.ModeSymlink != 0 && uid == d.adminUID {
		target, err := os.Readlink(path)
		if err != nil {
			return err
		}
		if !filepath.IsAbs(target) {
			target = filepath.Join(filepath.Dir(path), target)
		}
		if filepath.Clean(target) == d.unitPath() {
			return os.Remove(path)
		}
	}
	return fmt.Errorf("refusing to remove an unexpected boot entry %s", path)
}

func (d *relayDaemon) openManagementLock() (io.Closer, error) {
	path := d.lockPath()
	if err := d.trustedParents(path); err != nil {
		return nil, err
	}
	return d.lock(path, d.adminUID)
}

func (d *relayDaemon) inspectState(path string, account relayDaemonAccount) (bool, peer.ID, error) {
	for _, managed := range []string{d.binaryPath, d.configPath, d.unitPath(), d.lockPath(), d.enablePath()} {
		if path == managed || path == filepath.Dir(managed) {
			return false, "", fmt.Errorf("relay state path %s conflicts with a system management path", path)
		}
	}
	if err := d.trustedParents(path); err != nil {
		return false, "", err
	}
	info, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, "", nil
	}
	if err != nil {
		return false, "", err
	}
	uid, gid, _ := d.fileIDs(info)
	if !info.IsDir() || info.Mode().Perm() != 0o700 || uid != account.uid || gid != account.gid {
		return false, "", fmt.Errorf("refusing relay state %s; it must be a private directory owned by the dedicated %s account", path, relayDaemonUser)
	}
	entries, err := os.ReadDir(path)
	if err != nil || len(entries) == 0 {
		return true, "", err
	}
	if len(entries) != 1 || entries[0].Name() != "link.key" {
		return false, "", fmt.Errorf("refusing unknown files in existing relay state %s", path)
	}
	key, err := entries[0].Info()
	if err != nil {
		return false, "", err
	}
	uid, gid, links := d.fileIDs(key)
	if !key.Mode().IsRegular() || key.Mode().Perm()&0o077 != 0 || uid != account.uid || gid != account.gid || links != 1 {
		return false, "", fmt.Errorf("refusing unsafe identity in existing relay state %s", path)
	}
	id, err := readRelayPeerID(path)
	return true, id, err
}

func createRelayDaemonState(path string, account relayDaemonAccount) error {
	if err := os.Mkdir(path, 0o700); err != nil {
		return err
	}
	if err := os.Chmod(path, 0o700); err != nil {
		return errors.Join(err, os.Remove(path))
	}
	if err := os.Chown(path, account.uid, account.gid); err != nil {
		return errors.Join(err, os.Remove(path))
	}
	return nil
}

func checkRelayDaemonPort(ctx context.Context, options startOptions) error {
	addr := options.config.ListenAddr
	if len(addr) != 2 || addr[1].Code() != ma.P_TCP || addr[1].Value() == "0" {
		return errors.New("the managed relay needs a fixed TCP listen port")
	}
	network := "tcp4"
	if addr[0].Code() == ma.P_IP6 {
		network = "tcp6"
	}
	endpoint := net.JoinHostPort(addr[0].Value(), addr[1].Value())
	var listen net.ListenConfig
	listener, err := listen.Listen(ctx, network, endpoint)
	if err != nil {
		return fmt.Errorf("relay cannot bind %s: %w", endpoint, err)
	}
	return listener.Close()
}

type relayDaemonNewFile struct {
	path string
	info os.FileInfo
}

type relayDaemonNewFiles []relayDaemonNewFile

func (files *relayDaemonNewFiles) mkdir(path string) error {
	if err := os.Mkdir(path, 0o755); err != nil {
		return err
	}
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	*files = append(*files, relayDaemonNewFile{path, info})
	return os.Chmod(path, 0o755)
}

func (files *relayDaemonNewFiles) write(path string, mode os.FileMode, src io.Reader) error {
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return err
	}
	info, err := file.Stat()
	if err == nil {
		*files = append(*files, relayDaemonNewFile{path, info})
		_, err = io.Copy(file, src)
	}
	if err == nil {
		err = file.Chmod(mode)
	}
	if err == nil {
		err = file.Sync()
	}
	return errors.Join(err, file.Close())
}

func (files relayDaemonNewFiles) remove() error {
	var result error
	for i := len(files) - 1; i >= 0; i-- {
		file := files[i]
		info, err := os.Lstat(file.path)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err == nil {
			if !os.SameFile(info, file.info) {
				err = fmt.Errorf("refusing to remove replaced path %s", file.path)
			} else {
				err = os.Remove(file.path)
			}
		}
		result = errors.Join(result, err)
	}
	return result
}
