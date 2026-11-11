package files

import (
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

// driveDetail is a drive's label and its medium. A network drive's label is
// not read: the read waits for its server.
func driveDetail(root string) (string, *protocol.FileBrowserMedium) {
	pointer, err := windows.UTF16PtrFromString(root)
	if err != nil {
		return "", nil
	}
	var medium protocol.FileBrowserMedium
	switch windows.GetDriveType(pointer) {
	case windows.DRIVE_FIXED:
		medium = protocol.FileBrowserMediumInternal
	case windows.DRIVE_REMOVABLE, windows.DRIVE_CDROM:
		medium = protocol.FileBrowserMediumExternal
	case windows.DRIVE_REMOTE:
		medium = protocol.FileBrowserMediumNetwork
		return "", &medium
	default:
		return "", nil
	}
	label := make([]uint16, windows.MAX_PATH+1)
	if windows.GetVolumeInformation(pointer, &label[0], uint32(len(label)), nil, nil, nil, nil, 0) != nil {
		return "", &medium
	}
	return windows.UTF16ToString(label), &medium
}
