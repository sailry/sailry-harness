package node

import (
	"strings"
	"unicode"
	"unsafe"

	"golang.org/x/sys/windows"
)

func workspaceNameKey(parent, name string) string {
	// Windows compares ordinal uppercase characters. Keep ASCII lowercase in
	// the key so common lowercase paths need no directory query.
	folded := strings.Map(func(r rune) rune {
		r = unicode.ToUpper(r)
		if 'A' <= r && r <= 'Z' {
			return r + ('a' - 'A')
		}
		return r
	}, name)
	if folded == name {
		return name
	}
	path, err := windows.UTF16PtrFromString(parent)
	if err != nil {
		return name
	}
	handle, err := windows.CreateFile(path, windows.FILE_READ_ATTRIBUTES,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil,
		windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return name
	}
	defer windows.CloseHandle(handle)
	var flags uint32
	err = windows.GetFileInformationByHandleEx(handle, windows.FileCaseSensitiveInfo, (*byte)(unsafe.Pointer(&flags)), uint32(unsafe.Sizeof(flags)))
	switch err {
	case nil:
		if flags&windows.FILE_CS_FLAG_CASE_SENSITIVE_DIR != 0 {
			return name
		}
	case windows.ERROR_INVALID_FUNCTION, windows.ERROR_NOT_SUPPORTED, windows.ERROR_INVALID_PARAMETER:
		// Filesystems without directory case-sensitivity flags use Win32 lookup.
	default:
		return name
	}
	return folded
}
