//go:build unix

package git

import (
	"fmt"
	"os"
	"reflect"
	"syscall"

	idxfmt "github.com/go-git/go-git/v5/plumbing/format/index"
)

// indexStat reports whether a worktree file still has the stat data Git
// recorded in its index entry: size, modification and change times, device
// and inode, each as truncated by the index format.
func indexStat(entry *idxfmt.Entry, info os.FileInfo) bool {
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok {
		return false
	}
	value := reflect.ValueOf(stat).Elem()
	field := value.FieldByName("Ctim")
	if !field.IsValid() {
		field = value.FieldByName("Ctimespec")
	}
	changed, ok := field.Interface().(syscall.Timespec)
	if !ok {
		return false
	}
	seconds, nanoseconds := changed.Unix()
	modified := info.ModTime()
	// Git records st_dev; libgit2 records st_rdev.
	return entry.Size == uint32(info.Size()) &&
		entry.ModifiedAt.Unix() == int64(uint32(modified.Unix())) && entry.ModifiedAt.Nanosecond() == modified.Nanosecond() &&
		entry.CreatedAt.Unix() == int64(uint32(seconds)) && int64(entry.CreatedAt.Nanosecond()) == nanoseconds &&
		(entry.Dev == uint32(stat.Dev) || entry.Dev == uint32(stat.Rdev)) && entry.Inode == uint32(stat.Ino)
}

func fileIdentity(info os.FileInfo) string {
	if stat, ok := info.Sys().(*syscall.Stat_t); ok {
		value := reflect.ValueOf(stat).Elem()
		changed := value.FieldByName("Ctim")
		if !changed.IsValid() {
			changed = value.FieldByName("Ctimespec")
		}
		return fmt.Sprintf("%d:%d:%v", stat.Dev, stat.Ino, changed)
	}
	return ""
}
