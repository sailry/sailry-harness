package files

import (
	"os"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

// systemPlaces asks the shell for the account's known folders, which follow
// a folder the user or OneDrive moved; one it cannot name is the folder of
// that name under the home.
func systemPlaces(home string) []place {
	out := userPlaces("windows", home, os.Getenv)
	known := map[protocol.FileBrowserLocationKind]*windows.KNOWNFOLDERID{
		protocol.FileBrowserLocationKindDesktop:   windows.FOLDERID_Desktop,
		protocol.FileBrowserLocationKindDocuments: windows.FOLDERID_Documents,
		protocol.FileBrowserLocationKindDownloads: windows.FOLDERID_Downloads,
	}
	for i := range out {
		if path, err := windows.KnownFolderPath(known[out[i].kind], windows.KF_FLAG_DEFAULT); err == nil && path != "" {
			out[i].path = path
		}
	}
	return out
}
