// Portions adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
// Copyright 2026 Zavora Technologies Ltd.; Apache-2.0. See third_party_licenses/adk-LICENSE.
package process

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

func sandboxCommand(policy sandboxPolicy, program string, args []string, device string) (string, []string, error) {
	wrapper, err := trustedWrapper("/usr/bin/sandbox-exec")
	if err != nil {
		return "", nil, err
	}
	profile, err := sandboxProfileMacOS(policy, device)
	if err != nil {
		return "", nil, err
	}
	return wrapper, append([]string{"-p", profile, program}, args...), nil
}

func sandboxProfileMacOS(policy sandboxPolicy, device string) (string, error) {
	var profile strings.Builder
	profile.WriteString("(version 1)\n(deny default)\n(allow process-exec)\n(allow process-fork)\n(allow signal (target same-sandbox))\n(allow sysctl-read)\n(allow user-preference-read)\n(allow file-read-metadata)\n(allow file-read* (literal \"/\"))\n")
	rule := func(action, filter string, paths ...string) {
		profile.WriteString("(" + action)
		for _, path := range paths {
			fmt.Fprintf(&profile, " (%s %s)", filter, sandboxLiteral(path))
		}
		profile.WriteString(")\n")
	}
	rule("allow file-read*", "subpath", "/bin", "/sbin", "/usr", "/System", "/Library/Apple", "/Library/Frameworks", "/opt/homebrew", "/private/etc", "/private/var/db/dyld", "/private/var/db/timezone", "/private/var/select")
	rule("allow file-read*", "literal", "/dev/random", "/dev/urandom")
	rule("allow file-read* file-write-data", "literal", "/dev/null", "/dev/zero")
	rule("allow file-read* file-write-data file-ioctl", "literal", "/dev/dtracehelper")
	rule("allow file-read-data file-write-data", "subpath", "/dev/fd")
	rule("deny file-read*", "subpath", "/private/etc/master.passwd", "/private/etc/sudoers", "/private/etc/sudoers.d")
	rule("allow mach-lookup", "global-name", "com.apple.system.opendirectoryd.libinfo", "com.apple.system.opendirectoryd.membership", "com.apple.bsd.dirhelper", "com.apple.system.notification_center", "com.apple.system.logger", "com.apple.logd", "com.apple.cfprefsd.daemon", "com.apple.cfprefsd.agent")
	profile.WriteString("(allow ipc-posix-shm-read* (ipc-posix-name \"apple.shm.notification_center\") (ipc-posix-name-prefix \"apple.cfprefs.\"))\n")
	for _, entry := range orderedPaths(policy.paths) {
		if !utf8.ValidString(entry.path) {
			return "", sandboxFailure(fmt.Errorf("allowed path is not valid UTF-8"))
		}
		if entry.write {
			rule("allow file-read* file-write*", "subpath", entry.path)
		} else {
			rule("allow file-read*", "subpath", entry.path)
			parent, ok := effectivePath(policy.paths, entry.path, true)
			effective, _ := effectivePath(policy.paths, entry.path, false)
			if ok && parent && !effective {
				rule("deny file-write*", "subpath", entry.path)
			}
		}
	}
	for _, path := range pinnedPaths(policy.paths) {
		rule("deny file-write-unlink", "literal", path)
	}
	profile.WriteString("(deny file-write* (regex #\"(^|/)\\.[gG][iI][tT](/|$)\"))\n")
	for _, path := range policy.protected {
		rule("deny file-write*", "subpath", path)
		if filepath.Base(path) == "worktrees" && filepath.Base(filepath.Dir(path)) == ".sailry" {
			rule("deny file-write-unlink", "literal", filepath.Dir(path))
		}
	}
	if device != "" {
		resolved, err := filepath.EvalSymlinks(device)
		if err != nil {
			return "", sandboxFailure(err)
		}
		info, err := os.Stat(resolved)
		if err != nil || filepath.Dir(resolved) != "/dev" || info.Mode()&os.ModeCharDevice == 0 {
			return "", unavailable("native terminal device is invalid")
		}
		rule("allow file-read-data file-write-data file-ioctl", "literal", resolved)
	}
	return profile.String(), nil
}

func sandboxLiteral(value string) string {
	var text strings.Builder
	text.WriteByte('"')
	for _, r := range value {
		switch {
		case r == '\\' || r == '"':
			text.WriteByte('\\')
			text.WriteRune(r)
		case r < 32 || r == 127:
			fmt.Fprintf(&text, "\\x%02x", r)
		default:
			text.WriteRune(r)
		}
	}
	text.WriteByte('"')
	return text.String()
}
