//go:build unix

package git

import (
	"fmt"
	"os"
	"reflect"
	"syscall"
)

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
