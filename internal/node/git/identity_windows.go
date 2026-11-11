//go:build windows

package git

import (
	"fmt"
	"os"
	"syscall"
	"time"

	idxfmt "github.com/go-git/go-git/v5/plumbing/format/index"
)

// indexStat reports whether a worktree file still has the stat data Git
// recorded in its index entry. Git for Windows and libgit2 record the
// creation time where other systems record the change time.
func indexStat(entry *idxfmt.Entry, info os.FileInfo) bool {
	stat, ok := info.Sys().(*syscall.Win32FileAttributeData)
	if !ok {
		return false
	}
	created, modified := time.Unix(0, stat.CreationTime.Nanoseconds()), info.ModTime()
	return entry.Size == uint32(info.Size()) &&
		entry.ModifiedAt.Unix() == int64(uint32(modified.Unix())) && entry.ModifiedAt.Nanosecond() == modified.Nanosecond() &&
		entry.CreatedAt.Unix() == int64(uint32(created.Unix())) && entry.CreatedAt.Nanosecond() == created.Nanosecond()
}

func fileIdentity(info os.FileInfo) string {
	if stat, ok := info.Sys().(*syscall.Win32FileAttributeData); ok {
		return fmt.Sprintf("%d:%d", stat.CreationTime.HighDateTime, stat.CreationTime.LowDateTime)
	}
	return ""
}
