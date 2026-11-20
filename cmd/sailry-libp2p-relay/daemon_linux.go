//go:build linux && !android

package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/user"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"

	"golang.org/x/sys/unix"
)

const relayDaemonHome = "/nonexistent"

func runPlatformDaemon(ctx context.Context, action string, args []string, output io.Writer) error {
	if action != "status" && os.Geteuid() != 0 {
		return fmt.Errorf("daemon %s requires root", action)
	}
	d := relayDaemon{
		binaryPath: "/usr/local/bin/sailry-relay",
		configPath: "/etc/sailry-relay/" + relayConfigName,
		ctl: func(ctx context.Context, args ...string) (string, error) {
			return relaySystemCommand(ctx, "bin", "systemctl", append([]string{"--system", "--no-pager", "--no-ask-password"}, args...)...)
		},
		root:         "/",
		sourceBinary: "/proc/self/exe",
		fileIDs:      relayDaemonFileIDs,
		lock:         lockRelayDaemon,
		account:      relaySystemAccount,
		ready:        waitRelayReady,
	}
	return d.manage(ctx, action, args, output)
}

func relayDaemonFileIDs(info os.FileInfo) (uid, gid int, links uint64) {
	stat := info.Sys().(*syscall.Stat_t)
	return int(stat.Uid), int(stat.Gid), uint64(stat.Nlink)
}

func lockRelayDaemon(path string, adminUID int) (io.Closer, error) {
	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE|unix.O_NOFOLLOW|unix.O_NONBLOCK, 0o600)
	if err != nil {
		return nil, fmt.Errorf("open relay management lock: %w", err)
	}
	info, err := file.Stat()
	if err == nil {
		uid, _, links := relayDaemonFileIDs(info)
		if !info.Mode().IsRegular() || info.Mode().Perm() != 0o600 || uid != adminUID || links != 1 {
			err = fmt.Errorf("refusing unsafe relay management lock %s", path)
		}
	}
	if err == nil {
		err = unix.Flock(int(file.Fd()), unix.LOCK_EX|unix.LOCK_NB)
		if errors.Is(err, unix.EWOULDBLOCK) {
			err = errors.New("another relay management command is in progress")
		}
	}
	if err != nil {
		file.Close()
		return nil, err
	}
	return file, nil
}

func relaySystemAccount(ctx context.Context, create bool) (relayDaemonAccount, error) {
	account, err := user.Lookup(relayDaemonUser)
	var missing user.UnknownUserError
	if errors.As(err, &missing) {
		if !create {
			return relayDaemonAccount{}, errRelayDaemonAccountMissing
		}
		if _, err := relaySystemCommand(ctx, "sbin", "useradd", "--system", "--user-group", "--no-create-home", "--home-dir", relayDaemonHome, "--shell", "/sbin/nologin", relayDaemonUser); err != nil {
			return relayDaemonAccount{}, err
		}
		account, err = user.Lookup(relayDaemonUser)
	}
	if err != nil {
		return relayDaemonAccount{}, err
	}
	uid, uidErr := strconv.Atoi(account.Uid)
	gid, gidErr := strconv.Atoi(account.Gid)
	group, groupErr := user.LookupGroup(relayDaemonUser)
	if uidErr != nil || gidErr != nil || groupErr != nil || uid <= 0 || gid <= 0 || account.Username != relayDaemonUser || account.HomeDir != relayDaemonHome || group.Gid != account.Gid {
		return relayDaemonAccount{}, fmt.Errorf("existing %s account is not the dedicated non-root relay account", relayDaemonUser)
	}
	return relayDaemonAccount{uid: uid, gid: gid}, nil
}

func relaySystemCommand(ctx context.Context, directory, name string, args ...string) (string, error) {
	for _, prefix := range []string{"/usr", "/"} {
		path := filepath.Join(prefix, directory, name)
		if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
			continue
		} else if err != nil {
			return "", err
		}
		command := exec.CommandContext(ctx, path, args...)
		command.Env = []string{"PATH=/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL=C"}
		data, err := command.CombinedOutput()
		text := strings.TrimSpace(string(data))
		if err != nil {
			if text != "" {
				return text, fmt.Errorf("%s %s: %w: %s", name, strings.Join(args, " "), err, text)
			}
			return text, fmt.Errorf("%s %s: %w", name, strings.Join(args, " "), err)
		}
		return text, nil
	}
	return "", fmt.Errorf("%s is required for relay daemon management", name)
}
