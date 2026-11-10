package files

import (
	"github.com/sailry/sailry-harness/internal/protocol"
	"os"
	"strings"
)

func browserVolumes() ([]protocol.FileBrowserLocation, error) {
	var out []protocol.FileBrowserLocation
	for drive := 'A'; drive <= 'Z'; drive++ {
		path := string(drive) + ":\\"
		if info, err := os.Stat(path); err == nil && info.IsDir() {
			out = append(out, protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: strings.TrimSuffix(path, "\\"), Path: path})
		}
	}
	return out, nil
}
