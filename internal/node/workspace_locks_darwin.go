package node

import (
	"strings"
	"unicode/utf8"

	"golang.org/x/sys/unix"
	"golang.org/x/text/cases"
	"golang.org/x/text/unicode/norm"
)

var workspaceCaseFold = cases.Fold()

func workspaceNameKey(parent, name string) string {
	var folded string
	if len(name) == utf8.RuneCountInString(name) {
		folded = strings.ToLower(name)
	} else {
		folded = workspaceCaseFold.String(name)
	}
	if folded != name {
		const pcCaseSensitive = 11 // Darwin's _PC_CASE_SENSITIVE from sys/unistd.h.
		if sensitive, err := unix.Pathconf(parent, pcCaseSensitive); err == nil && sensitive == 0 {
			name = folded
		}
	}
	if !norm.NFC.IsNormalString(name) {
		var fs unix.Statfs_t
		if unix.Statfs(parent, &fs) == nil {
			switch unix.ByteSliceToString(fs.Fstypename[:]) {
			case "apfs", "hfs":
				return norm.NFC.String(name)
			}
		}
	}
	return name
}
