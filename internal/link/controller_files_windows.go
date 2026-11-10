//go:build windows

package link

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

const controllerFileAllAccess = 0x001f01ff

func prepareControllerFiles(path string) (string, *os.File, error) {
	if err := privateControllerDirectory(path); err != nil {
		return "", nil, err
	}
	directory, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", nil, err
	}
	if err := validateControllerIdentity(directory); err != nil {
		return "", nil, err
	}
	lockPath := filepath.Join(directory, "node.lock")
	name, err := controllerWindowsName(lockPath)
	if err != nil {
		return "", nil, err
	}
	handle, err := windows.CreateFile(name, windows.GENERIC_READ|windows.GENERIC_WRITE|windows.READ_CONTROL,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_ALWAYS,
		windows.FILE_ATTRIBUTE_NORMAL|windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return "", nil, err
	}
	lock := os.NewFile(uintptr(handle), lockPath)
	if err := validateWindowsControllerHandle(handle, false); err != nil {
		_ = lock.Close()
		return "", nil, err
	}
	if err := windows.LockFileEx(handle, windows.LOCKFILE_FAIL_IMMEDIATELY|windows.LOCKFILE_EXCLUSIVE_LOCK,
		0, ^uint32(0), ^uint32(0), new(windows.Overlapped)); err != nil {
		_ = lock.Close()
		if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
			return "", nil, errors.New("controller profile is already in use")
		}
		return "", nil, err
	}
	return directory, lock, nil
}

func privateControllerDirectory(path string) error {
	if _, err := os.Lstat(path); err == nil {
		return validateWindowsControllerPath(path, true)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	parent := filepath.Dir(path)
	if parent == path || filepath.Base(path) == "." {
		return errors.New("controller directory has no parent")
	}
	if err := os.MkdirAll(parent, 0o700); err != nil {
		return err
	}
	canonical, err := filepath.EvalSymlinks(parent)
	if err != nil {
		return err
	}
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return err
	}
	sid := user.User.Sid.String()
	security, err := windows.SecurityDescriptorFromString("O:" + sid + "D:P(A;OICI;FA;;;" + sid + ")")
	if err != nil {
		return err
	}
	attributes := windows.SecurityAttributes{
		Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: security,
	}
	name, err := controllerWindowsName(filepath.Join(canonical, filepath.Base(path)))
	if err != nil {
		return err
	}
	if err := windows.CreateDirectory(name, &attributes); err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
		return err
	}
	runtime.KeepAlive(user)
	runtime.KeepAlive(security)
	return validateWindowsControllerPath(path, true)
}

func readControllerFile(path string) (*os.File, error) {
	name, err := controllerWindowsName(path)
	if err != nil {
		return nil, err
	}
	handle, err := windows.CreateFile(name, windows.GENERIC_READ|windows.READ_CONTROL,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING,
		windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(handle), path)
	if err := validateWindowsControllerHandle(handle, false); err != nil {
		_ = file.Close()
		return nil, err
	}
	return file, nil
}

func validateControllerIdentity(directory string) error {
	return validateExistingControllerFile(filepath.Join(directory, "link.key"))
}

func validateExistingControllerFile(path string) error {
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	return validateWindowsControllerPath(path, false)
}

func validateWindowsControllerPath(path string, directory bool) error {
	name, err := controllerWindowsName(path)
	if err != nil {
		return err
	}
	handle, err := windows.CreateFile(name, windows.READ_CONTROL|windows.FILE_READ_ATTRIBUTES,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING,
		windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	return validateWindowsControllerHandle(handle, directory)
}

func validateWindowsControllerHandle(handle windows.Handle, directory bool) error {
	var information windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &information); err != nil {
		return err
	}
	if information.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return errors.New("profile storage must not be a reparse point")
	}
	if (information.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY != 0) != directory {
		return errors.New("profile storage has an unexpected file type")
	}
	if !directory && information.NumberOfLinks != 1 {
		return errors.New("profile storage must not be hard-linked")
	}
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return err
	}
	security, err := windows.GetSecurityInfo(handle, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		return err
	}
	owner, _, err := security.Owner()
	if err != nil {
		return err
	}
	if owner == nil || !owner.Equals(user.User.Sid) {
		return errors.New("profile storage must belong to the current user")
	}
	dacl, _, err := security.DACL()
	if err != nil || dacl == nil || !security.IsValid() {
		return errors.New("profile storage requires a private access list")
	}
	control, _, err := security.Control()
	if err != nil {
		return err
	}
	if directory && control&windows.SE_DACL_PROTECTED == 0 {
		return errors.New("controller directory must exclude inherited access")
	}
	userAccess := false
	for index := uint32(0); index < uint32(dacl.AceCount); index++ {
		var ace *windows.ACCESS_ALLOWED_ACE
		if err := windows.GetAce(dacl, index, &ace); err != nil {
			return err
		}
		sidOffset := unsafe.Offsetof(ace.SidStart)
		if ace.Header.AceType != windows.ACCESS_ALLOWED_ACE_TYPE || uintptr(ace.Header.AceSize) < sidOffset+8 {
			return errors.New("profile storage has an unsupported access entry")
		}
		sid := (*windows.SID)(unsafe.Pointer(&ace.SidStart))
		if uintptr(sid.Len()) > uintptr(ace.Header.AceSize)-sidOffset || !sid.IsValid() {
			return errors.New("profile storage has an invalid access entry")
		}
		current := sid.Equals(user.User.Sid)
		if !current && !sid.IsWellKnown(windows.WinLocalSystemSid) {
			return errors.New("profile storage permissions must exclude other users")
		}
		flags := ace.Header.AceFlags
		fullAccess := uint32(ace.Mask)&controllerFileAllAccess == controllerFileAllAccess || uint32(ace.Mask)&windows.GENERIC_ALL != 0
		inheritable := flags&(windows.OBJECT_INHERIT_ACE|windows.CONTAINER_INHERIT_ACE) ==
			windows.OBJECT_INHERIT_ACE|windows.CONTAINER_INHERIT_ACE && flags&windows.NO_PROPAGATE_INHERIT_ACE == 0
		userAccess = userAccess || current && fullAccess && flags&windows.INHERIT_ONLY_ACE == 0 && (!directory || inheritable)
	}
	runtime.KeepAlive(security)
	runtime.KeepAlive(user)
	if !userAccess {
		if directory {
			return errors.New("controller directory must grant inheritable current-user access")
		}
		return errors.New("profile storage must grant full current-user access")
	}
	return nil
}

func replaceControllerFile(source, target string) error {
	if err := validateWindowsControllerPath(source, false); err != nil {
		return err
	}
	if err := validateExistingControllerFile(target); err != nil {
		return err
	}
	sourceName, err := controllerWindowsName(source)
	if err != nil {
		return err
	}
	targetName, err := controllerWindowsName(target)
	if err != nil {
		return err
	}
	if err := windows.SetFileAttributes(sourceName, windows.FILE_ATTRIBUTE_NORMAL); err != nil {
		return err
	}
	return windows.MoveFileEx(sourceName, targetName, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH)
}

func syncControllerDirectory(string) error { return nil }

func controllerWindowsName(path string) (*uint16, error) {
	if filepath.IsAbs(path) && !strings.HasPrefix(path, `\\?\`) && !strings.HasPrefix(path, `\\.\`) {
		if strings.HasPrefix(path, `\\`) {
			path = `\\?\UNC\` + strings.TrimPrefix(path, `\\`)
		} else {
			path = `\\?\` + path
		}
	}
	return windows.UTF16PtrFromString(path)
}
