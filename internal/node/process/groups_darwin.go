package process

import (
	"errors"
	"strconv"

	"golang.org/x/sys/unix"
)

// processStarted is the start time the kernel recorded for a live process.
func processStarted(id int) (string, error) {
	process, err := unix.SysctlKinfoProc("kern.proc.pid", id)
	if err != nil {
		return "", err
	}
	if int(process.Proc.P_pid) != id {
		return "", errors.New("process is not running")
	}
	started := process.Proc.P_starttime
	return strconv.FormatInt(started.Sec, 10) + "." + strconv.FormatInt(int64(started.Usec), 10), nil
}
