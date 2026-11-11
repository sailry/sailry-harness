package process

import (
	"errors"
	"os"
	"strconv"
	"strings"
)

// processStarted is the boot and the start tick the kernel recorded for a
// live process.
func processStarted(id int) (string, error) {
	stat, err := os.ReadFile("/proc/" + strconv.Itoa(id) + "/stat")
	if err != nil {
		return "", err
	}
	// The command name may hold spaces and parentheses; fields resume after its last one.
	end := strings.LastIndexByte(string(stat), ')')
	fields := strings.Fields(string(stat[end+1:]))
	// Field 22 of the line is the start time; fields[0] is field 3.
	if end < 0 || len(fields) < 20 {
		return "", errors.New("process status is invalid")
	}
	boot, err := os.ReadFile("/proc/sys/kernel/random/boot_id")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(boot)) + ":" + fields[19], nil
}
