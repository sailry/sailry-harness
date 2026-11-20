package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"debug/buildinfo"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/link"
)

const relayProgram = "github.com/sailry/sailry-harness/cmd/sailry-libp2p-relay"
const maxLocalRelayBinary = 128 << 20

func (d *relayDaemon) update(ctx context.Context, args []string, output io.Writer) error {
	if len(args) != 0 && (len(args) != 3 || args[1] != "--sha256") {
		return errors.New("usage: sailry-relay daemon update [<binary> --sha256 <digest>]")
	}
	options, _, err := readRelayConfig(d.configPath)
	if err != nil {
		return err
	}
	identity, err := readRelayPeerID(options.directory)
	if err != nil {
		return fmt.Errorf("read existing relay identity before update: %w", err)
	}
	stage, err := os.MkdirTemp(filepath.Dir(d.binaryPath), ".sailry-relay-update-")
	if err != nil {
		return fmt.Errorf("stage relay update: %w", err)
	}
	defer os.RemoveAll(stage)
	var candidate, version string
	if len(args) == 0 {
		// Only the installed executable can report the currently installed version.
		executable, err := os.Executable()
		if err != nil {
			return err
		}
		installed, err := os.Stat(d.binaryPath)
		if err != nil {
			return err
		}
		current, err := os.Stat(executable)
		if err != nil || !os.SameFile(current, installed) {
			return fmt.Errorf("run the installed command %s daemon update", d.binaryPath)
		}
		candidate, version, err = officialRelayReleases().latest(ctx, link.ProductVersion, runtime.GOARCH, stage)
		if err != nil {
			return err
		}
		if candidate == "" {
			_, err := fmt.Fprintf(output, "No newer relay release (installed %s, latest %s).\n", link.ProductVersion, version)
			return err
		}
	} else {
		candidate = filepath.Join(stage, "sailry-relay")
		if err := copyLocalRelayBinary(args[0], candidate, args[2]); err != nil {
			return err
		}
	}
	if err := validateRelayProgram(candidate, runtime.GOARCH); err != nil {
		return err
	}
	if err := os.Chmod(candidate, 0o755); err != nil {
		return err
	}
	properties, err := d.ownedUnit(ctx)
	if err != nil {
		return err
	}
	state := properties["ActiveState"]
	switch state {
	case "active":
	case "inactive", "failed":
		if err := checkRelayDaemonPort(ctx, options); err != nil {
			return err
		}
	default:
		return fmt.Errorf("relay service is %s; wait for it to settle before updating", state)
	}
	restart := func(ctx context.Context) error {
		if state == "failed" {
			if _, err := d.ctl(ctx, "reset-failed", relayDaemonUnit); err != nil {
				return err
			}
		}
		return d.activate(ctx, "restart", options, identity)
	}
	recoverService := func(ctx context.Context) error {
		if state != "active" {
			_, err := d.ctl(ctx, "stop", relayDaemonUnit)
			return err
		}
		_, _ = d.ctl(ctx, "reset-failed", relayDaemonUnit)
		return d.activate(ctx, "restart", options, identity)
	}
	if err := applyRelayBinary(ctx, d.binaryPath, candidate, restart, recoverService); err != nil {
		return err
	}
	if version == "" {
		version = "from local binary"
	}
	_, err = fmt.Fprintf(output, "Updated relay %s; service is online with PeerID %s.\n", version, identity)
	return err
}

func validateRelayProgram(path, arch string) error {
	info, err := buildinfo.ReadFile(path)
	if err != nil || info.Path != relayProgram {
		return errors.New("update is not a Sailry relay Go executable")
	}
	var target, cpu string
	for _, setting := range info.Settings {
		switch setting.Key {
		case "GOOS":
			target = setting.Value
		case "GOARCH":
			cpu = setting.Value
		}
	}
	if target != "linux" || cpu != arch {
		return fmt.Errorf("update targets %s/%s, expected linux/%s", target, cpu, arch)
	}
	return nil
}

func copyLocalRelayBinary(source, destination, digest string) error {
	wanted, err := hex.DecodeString(digest)
	if err != nil || len(wanted) != sha256.Size {
		return errors.New("expected a SHA-256 digest")
	}
	in, info, err := openRelayRegularFile(source, maxLocalRelayBinary)
	if err != nil {
		return fmt.Errorf("open local relay binary %q: %w", source, err)
	}
	defer in.Close()
	if info.Size() == 0 {
		return errors.New("local relay binary is empty")
	}
	out, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o755)
	if err != nil {
		return err
	}
	hash := sha256.New()
	copyErr := copyRelayRegularFile(io.MultiWriter(out, hash), in, info.Size())
	if copyErr == nil && !bytes.Equal(hash.Sum(nil), wanted) {
		copyErr = errors.New("local relay binary checksum does not match")
	}
	if copyErr == nil {
		copyErr = out.Sync()
	}
	if err := errors.Join(copyErr, out.Close()); err != nil {
		return errors.Join(err, os.Remove(destination))
	}
	return nil
}

func copyRelayBackup(installed, staged string) error {
	in, info, err := openRelayRegularFile(installed, maxLocalRelayBinary)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(staged, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return err
	}
	copyErr := copyRelayRegularFile(out, in, info.Size())
	if copyErr == nil {
		copyErr = out.Chmod(info.Mode().Perm())
	}
	if copyErr == nil {
		copyErr = out.Sync()
	}
	if err := errors.Join(copyErr, out.Close()); err != nil {
		return errors.Join(err, os.Remove(staged))
	}
	return nil
}

func applyRelayBinary(ctx context.Context, installed, candidate string, activate, recoverService func(context.Context) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	backup := installed + ".previous"
	if info, err := os.Lstat(backup); err == nil {
		if !info.Mode().IsRegular() || info.Mode().Perm()&0o022 != 0 {
			return fmt.Errorf("refusing to replace unsafe relay backup %s", backup)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	stagedBackup := filepath.Join(filepath.Dir(candidate), "previous")
	if err := copyRelayBackup(installed, stagedBackup); err != nil {
		return fmt.Errorf("back up relay binary: %w", err)
	}
	defer os.Remove(stagedBackup)
	if err := os.Rename(stagedBackup, backup); err != nil {
		return fmt.Errorf("publish relay backup: %w", err)
	}
	if err := syncRelayProgramDirectory(installed); err != nil {
		return fmt.Errorf("persist relay backup: %w", err)
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := os.Rename(candidate, installed); err != nil {
		return fmt.Errorf("replace relay binary: %w", err)
	}
	activation := syncRelayProgramDirectory(installed)
	if activation == nil {
		activation = activate(ctx)
	}
	if err := activation; err != nil {
		cause := fmt.Errorf("updated relay did not become ready: %w", err)
		if err := os.Rename(backup, installed); err != nil {
			return errors.Join(cause, fmt.Errorf("restore relay binary from %s: %w", backup, err))
		}
		recovery, cancel := context.WithTimeout(context.WithoutCancel(ctx), 40*time.Second)
		defer cancel()
		if err := errors.Join(syncRelayProgramDirectory(installed), recoverService(recovery)); err != nil {
			return errors.Join(cause, fmt.Errorf("previous binary restored but relay service recovery failed: %w", err))
		}
		return fmt.Errorf("previous relay binary and service state restored: %w", cause)
	}
	return nil
}

func syncRelayProgramDirectory(installed string) error {
	directory, err := os.Open(filepath.Dir(installed))
	if err != nil {
		return err
	}
	return errors.Join(directory.Sync(), directory.Close())
}

func waitRelayReady(ctx context.Context, options startOptions, expected peer.ID) error {
	ctx, cancel := context.WithTimeout(ctx, 12*time.Second)
	defer cancel()
	address := options.config.ListenAddr
	if len(address) < 2 {
		return errors.New("relay listen address is missing")
	}
	switch address[0].Value() {
	case "0.0.0.0":
		address = ma.Join(ma.StringCast("/ip4/127.0.0.1"), address[1:])
	case "::":
		address = ma.Join(ma.StringCast("/ip6/::1"), address[1:])
	}
	if address[1].Value() == "0" {
		return errors.New("the managed relay needs a fixed listen port")
	}
	tick := time.NewTicker(250 * time.Millisecond)
	defer tick.Stop()
	var lastErr error
	for {
		id, err := readRelayPeerID(options.directory)
		if err != nil && expected != "" && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		lastErr = err
		if err == nil {
			if expected != "" && id != expected {
				return errors.New("relay PeerID changed; refusing to confirm readiness")
			}
			probe, stop := context.WithTimeout(ctx, 2*time.Second)
			_, lastErr = link.ProbeRelay(probe, address.String()+"/p2p/"+id.String())
			stop()
			if lastErr == nil {
				return nil
			}
		}
		select {
		case <-tick.C:
		case <-ctx.Done():
			return fmt.Errorf("relay did not answer an authenticated ping: %w", errors.Join(ctx.Err(), lastErr))
		}
	}
}
