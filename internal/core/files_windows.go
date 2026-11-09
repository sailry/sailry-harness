package core

import (
	"io/fs"
	"os"
	"syscall"
	"time"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

type nativeFileIdentity struct {
	Volume uint64   `json:"volume"`
	File   [16]byte `json:"file"`
}

func fileNativeIdentity(file *os.File) (nativeFileIdentity, error) {
	var attributes struct {
		Attributes uint32
		ReparseTag uint32
	}
	handle := windows.Handle(file.Fd())
	if err := windows.GetFileInformationByHandleEx(handle, windows.FileAttributeTagInfo, (*byte)(unsafe.Pointer(&attributes)), uint32(unsafe.Sizeof(attributes))); err != nil {
		return nativeFileIdentity{}, fileIOFault(err)
	}
	if attributes.Attributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return nativeFileIdentity{}, fileFault(protocol.ErrorCodeInvalidRequest, "filesystem reparse points are not allowed")
	}
	var identity struct {
		Volume uint64
		File   [16]byte
	}
	if err := windows.GetFileInformationByHandleEx(handle, windows.FileIdInfo, (*byte)(unsafe.Pointer(&identity)), uint32(unsafe.Sizeof(identity))); err != nil {
		return nativeFileIdentity{}, fileIOFault(err)
	}
	return nativeFileIdentity(identity), nil
}

func regularFileFlags() int   { return os.O_RDONLY }
func exclusiveFileFlags() int { return os.O_WRONLY | os.O_CREATE | os.O_EXCL }

func fileReadonly(metadata fs.FileInfo) bool {
	return metadata.Sys().(*syscall.Win32FileAttributeData).FileAttributes&windows.FILE_ATTRIBUTE_READONLY != 0
}

func fileNativeVersion(_ *os.File, metadata fs.FileInfo) (any, any, error) {
	data := metadata.Sys().(*syscall.Win32FileAttributeData)
	return fileVersionTime(time.Unix(0, data.CreationTime.Nanoseconds())), fileReadonly(metadata), nil
}

func syncFileDirectory(_ *os.Root) error { return nil }

func renameFileBetween(source *os.Root, from string, target *os.Root, to string) error {
	sourceDirectory, err := source.Open(".")
	if err != nil {
		return err
	}
	defer sourceDirectory.Close()
	targetDirectory, err := target.Open(".")
	if err != nil {
		return err
	}
	defer targetDirectory.Close()
	objectName, err := windows.NewNTUnicodeString(from)
	if err != nil {
		return err
	}
	attributes := windows.OBJECT_ATTRIBUTES{RootDirectory: windows.Handle(sourceDirectory.Fd()), ObjectName: objectName, Attributes: windows.OBJ_CASE_INSENSITIVE}
	attributes.Length = uint32(unsafe.Sizeof(attributes))
	var handle windows.Handle
	var status windows.IO_STATUS_BLOCK
	err = windows.NtCreateFile(&handle, windows.DELETE|windows.SYNCHRONIZE, &attributes, &status, nil, 0, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, windows.FILE_OPEN, windows.FILE_OPEN_REPARSE_POINT|windows.FILE_OPEN_FOR_BACKUP_INTENT|windows.FILE_SYNCHRONOUS_IO_NONALERT, 0, 0)
	if err != nil {
		return fileWindowsStatus(err)
	}
	defer windows.CloseHandle(handle)
	name, err := windows.UTF16FromString(to)
	if err != nil {
		return err
	}
	type renameInformation struct {
		ReplaceIfExists uint32
		RootDirectory   windows.Handle
		FileNameLength  uint32
		FileName        [1]uint16
	}
	var layout renameInformation
	length := len(name) - 1
	buffer := make([]byte, int(unsafe.Offsetof(layout.FileName))+2*length)
	information := (*renameInformation)(unsafe.Pointer(&buffer[0]))
	information.ReplaceIfExists = 1
	information.RootDirectory = windows.Handle(targetDirectory.Fd())
	information.FileNameLength = uint32(2 * length)
	copy(unsafe.Slice(&information.FileName[0], length), name[:length])
	err = windows.NtSetInformationFile(handle, &status, &buffer[0], uint32(len(buffer)), windows.FileRenameInformation)
	return fileWindowsStatus(err)
}

func fileWindowsStatus(err error) error {
	if status, ok := err.(windows.NTStatus); ok {
		return status.Errno()
	}
	return err
}
