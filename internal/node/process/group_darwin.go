package process

import (
	"golang.org/x/sys/unix"
	"os"
)

func nativeGroupSettled(id int) bool {
	if id <= 1 {
		return false
	}
	processes, err := unix.SysctlKinfoProcSlice("kern.proc.all")
	if err != nil {
		return false
	}
	for _, process := range processes {
		if int(process.Eproc.Pgid) == id {
			owner := uint32(os.Geteuid())
			if process.Proc.P_stat != 5 || process.Eproc.Ucred.Uid != owner || process.Eproc.Pcred.P_ruid != owner || process.Eproc.Pcred.P_svuid != owner {
				return false
			}
		}
	}
	return true
}
