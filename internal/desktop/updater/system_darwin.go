package updater

import (
	"os/exec"
	"strings"
	"unicode/utf8"
)

func systemVersion() (string, error) {
	output, err := exec.Command("/usr/bin/sw_vers", "-productVersion").Output()
	if err != nil {
		return "", failure("updates_system_unavailable", "macOS version could not be read")
	}
	if !utf8.Valid(output) {
		return "", failure("updates_system_unavailable", "macOS version is not UTF-8")
	}
	return strings.TrimSpace(string(output)), nil
}
