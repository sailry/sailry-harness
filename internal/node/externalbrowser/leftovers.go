package externalbrowser

import (
	"os"
	"strings"

	"github.com/shirou/gopsutil/v4/process"
)

// killProcessesUsing kills every process started with a user data directory
// at or below prefix and returns how many it found. Chromium passes the
// directory to each of its helper processes, and session profiles belong to
// this Node alone, so the command line identifies the whole tree even after
// its main process is gone.
func killProcessesUsing(prefix string) int {
	processes, err := process.Processes()
	if err != nil {
		return 0
	}
	want := "--user-data-dir=" + prefix
	found := 0
	for _, candidate := range processes {
		if int(candidate.Pid) == os.Getpid() {
			continue
		}
		arguments, err := candidate.CmdlineSlice()
		if err != nil {
			continue
		}
		for _, argument := range arguments {
			if strings.HasPrefix(argument, want) {
				found++
				_ = candidate.Kill()
				break
			}
		}
	}
	return found
}
