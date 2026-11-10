package files

import (
	"bufio"
	"github.com/sailry/sailry-harness/internal/protocol"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

func browserVolumes() ([]protocol.FileBrowserLocation, error) {
	file, err := os.Open("/proc/self/mountinfo")
	if err != nil {
		return nil, err
	}
	defer file.Close()
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 4096), 1<<20)
	var out []protocol.FileBrowserLocation
	decode := strings.NewReplacer(`\040`, " ", `\011`, "\t", `\012`, "\n", `\134`, `\`)
	for scanner.Scan() {
		fields := strings.Fields(scanner.Text())
		if len(fields) < 7 {
			continue
		}
		separator := 0
		for i, value := range fields {
			if value == "-" {
				separator = i
				break
			}
		}
		if separator == 0 || separator+2 >= len(fields) {
			continue
		}
		kind := fields[separator+1]
		switch kind {
		case "proc", "sysfs", "tmpfs", "devtmpfs", "devpts", "cgroup", "cgroup2", "securityfs", "pstore", "debugfs", "tracefs", "mqueue", "hugetlbfs", "fusectl", "configfs", "binfmt_misc", "nsfs":
			continue
		}
		path := decode.Replace(fields[4])
		if !utf8.ValidString(path) || !filepath.IsAbs(path) {
			continue
		}
		out = append(out, protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: decode.Replace(fields[separator+2]), Path: path})
	}
	return out, scanner.Err()
}
