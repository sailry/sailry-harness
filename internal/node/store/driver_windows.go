//go:build windows

package store

import (
	"os"

	"github.com/sailry/sailry-harness/internal/core"
	"golang.org/x/sys/windows"
)

func checkDatabaseLinks(path string, _ os.FileInfo) error {
	file, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return err
	}
	handle, err := windows.CreateFile(file, windows.GENERIC_READ,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	return checkDatabaseHandle(path, handle)
}

func createDatabaseFile(path string) error {
	if err := core.PrepareDatabase(path); err != nil {
		return err
	}
	file, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return err
	}
	handle, err := windows.CreateFile(file, windows.GENERIC_READ|windows.GENERIC_WRITE,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil, windows.OPEN_ALWAYS, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	return checkDatabaseHandle(path, handle)
}

func validateDatabasePrivacy(path string) error {
	return core.ValidateDatabase(path)
}

func checkDatabaseHandle(path string, handle windows.Handle) error {
	var information windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &information); err != nil {
		return err
	}
	if information.FileAttributes&(windows.FILE_ATTRIBUTE_DIRECTORY|windows.FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
		return &core.InvalidProfileError{Path: path, Reason: "database must be a regular file"}
	}
	if information.NumberOfLinks != 1 {
		return &core.InvalidProfileError{Path: path, Reason: "database must not be hard-linked"}
	}
	return nil
}
