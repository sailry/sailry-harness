//go:build windows

package core

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"golang.org/x/sys/windows"
)

const fileAllAccess = 0x001f01ff

var windowsFileOwner struct {
	once sync.Once
	err  error
}

// PrepareWindowsProcessFileOwner makes newly created storage belong to this user.
// Elevated tokens may default to the Administrators group. SQLite sidecars,
// identity files and atomic replacements inherit a directory's DACL, but their
// owner comes from the creating token, so setting directory ownership alone is
// insufficient. This changes no privileges, access lists or existing files.
func PrepareWindowsProcessFileOwner() error {
	windowsFileOwner.once.Do(func() {
		var token windows.Token
		if err := windows.OpenProcessToken(windows.CurrentProcess(), windows.TOKEN_QUERY|windows.TOKEN_ADJUST_DEFAULT, &token); err != nil {
			windowsFileOwner.err = err
			return
		}
		defer token.Close()
		user, err := token.GetTokenUser()
		if err != nil {
			windowsFileOwner.err = err
			return
		}
		owner := user.User.Sid
		windowsFileOwner.err = windows.SetTokenInformation(token, windows.TokenOwner, (*byte)(unsafe.Pointer(&owner)), uint32(unsafe.Sizeof(owner)))
		runtime.KeepAlive(user)
	})
	return windowsFileOwner.err
}

func PrivateDirectory(path string) error {
	if err := PrepareWindowsProcessFileOwner(); err != nil {
		return err
	}
	if _, err := os.Lstat(path); err == nil {
		return validateWindowsStorage(path, true)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	parent := filepath.Dir(path)
	if parent == path || filepath.Base(path) == "." {
		return invalidProfile(path, "data directory has no parent")
	}
	if err := os.MkdirAll(parent, 0o700); err != nil {
		return err
	}
	canonical, err := filepath.EvalSymlinks(parent)
	if err != nil {
		return err
	}
	canonical, err = filepath.Abs(canonical)
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
	name, err := windowsName(filepath.Join(canonical, filepath.Base(path)))
	if err != nil {
		return err
	}
	if err := windows.CreateDirectory(name, &attributes); err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
		return err
	}
	runtime.KeepAlive(user)
	runtime.KeepAlive(security)
	return validateWindowsStorage(path, true)
}

func openProfileLock(path string) (*os.File, error) {
	lockPath := filepath.Join(path, "node.lock")
	for _, name := range []string{lockPath, filepath.Join(path, "link.key")} {
		if err := validateExistingWindowsFile(name); err != nil {
			return nil, err
		}
	}
	name, err := windowsName(lockPath)
	if err != nil {
		return nil, err
	}
	handle, err := windows.CreateFile(name, windows.GENERIC_READ|windows.GENERIC_WRITE,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil,
		windows.OPEN_ALWAYS, windows.FILE_ATTRIBUTE_NORMAL|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return nil, err
	}
	lock := os.NewFile(uintptr(handle), lockPath)
	if err := windows.LockFileEx(handle, windows.LOCKFILE_FAIL_IMMEDIATELY|windows.LOCKFILE_EXCLUSIVE_LOCK,
		0, ^uint32(0), ^uint32(0), new(windows.Overlapped)); err != nil {
		_ = lock.Close()
		if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
			return nil, &ProfileInUseError{Path: path}
		}
		return nil, err
	}
	return lock, nil
}

func unlockProfile(lock *os.File) error {
	return windows.UnlockFileEx(windows.Handle(lock.Fd()), 0, ^uint32(0), ^uint32(0), new(windows.Overlapped))
}

// PrepareDatabase checks inherited privacy before the database engine can create sidecars.
func PrepareDatabase(path string) error {
	if err := PrepareWindowsProcessFileOwner(); err != nil {
		return err
	}
	parent := filepath.Dir(path)
	if parent == path {
		return invalidProfile(path, "database directory is missing")
	}
	if err := validateWindowsStorage(parent, true); err != nil {
		return err
	}
	if err := ValidateDatabase(path); err != nil {
		return err
	}
	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
	if err == nil {
		if err := file.Close(); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrExist) {
		return err
	}
	return validateWindowsStorage(path, false)
}

func ValidateDatabase(path string) error {
	for _, file := range DatabaseFiles(path) {
		if err := validateExistingWindowsFile(file); err != nil {
			return err
		}
	}
	return validateExistingWindowsFile(path + "-journal")
}

func validateExistingWindowsFile(path string) error {
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	return validateWindowsStorage(path, false)
}

// validateWindowsStorage accepts storage owned by the current user whose access
// list grants only that user and System. Storage the user owns but whose access
// list is broader, for example after Explorer converted inherited permissions or
// a copy kept the source access list, is narrowed to the private list Sailry
// creates. Storage owned by anyone else is refused unchanged.
func validateWindowsStorage(path string, directory bool) error {
	handle, information, err := openWindowsStorage(path, directory, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	reason, owned, err := windowsStorageAccess(handle, directory)
	if err != nil || reason == "" {
		return err
	}
	if !owned {
		return invalidProfile(path, reason)
	}
	return restrictWindowsStorage(path, directory, information)
}

func openWindowsStorage(path string, directory bool, access uint32) (windows.Handle, windows.ByHandleFileInformation, error) {
	var information windows.ByHandleFileInformation
	name, err := windowsName(path)
	if err != nil {
		return 0, information, err
	}
	handle, err := windows.CreateFile(name, windows.READ_CONTROL|windows.FILE_READ_ATTRIBUTES|access,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING,
		windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return 0, information, err
	}
	if err := windows.GetFileInformationByHandle(handle, &information); err != nil {
		_ = windows.CloseHandle(handle)
		return 0, information, err
	}
	reason := ""
	switch {
	case information.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0:
		reason = "profile storage must not be a reparse point"
	case (information.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY != 0) != directory:
		reason = "profile storage has an unexpected file type"
	case !directory && information.NumberOfLinks != 1:
		reason = "profile storage must not be hard-linked"
	}
	if reason != "" {
		_ = windows.CloseHandle(handle)
		return 0, information, invalidProfile(path, reason)
	}
	return handle, information, nil
}

// restrictWindowsStorage reopens the same object with WRITE_DAC, which its owner
// always holds, and replaces the access list before checking it again.
func restrictWindowsStorage(path string, directory bool, expected windows.ByHandleFileInformation) error {
	handle, information, err := openWindowsStorage(path, directory, windows.WRITE_DAC)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	if information.VolumeSerialNumber != expected.VolumeSerialNumber ||
		information.FileIndexHigh != expected.FileIndexHigh || information.FileIndexLow != expected.FileIndexLow {
		return invalidProfile(path, "profile storage changed during validation")
	}
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return err
	}
	inheritance := ""
	if directory {
		inheritance = "OICI"
	}
	security, err := windows.SecurityDescriptorFromString("D:P(A;" + inheritance + ";FA;;;" + user.User.Sid.String() + ")")
	if err != nil {
		return err
	}
	dacl, _, err := security.DACL()
	if err != nil {
		return err
	}
	if err := windows.SetSecurityInfo(handle, windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION, nil, nil, dacl, nil); err != nil {
		return err
	}
	runtime.KeepAlive(security)
	runtime.KeepAlive(user)
	reason, _, err := windowsStorageAccess(handle, directory)
	if err != nil {
		return err
	}
	if reason != "" {
		return invalidProfile(path, reason)
	}
	return nil
}

// windowsStorageAccess reports why storage is not private. owned is true when
// the current user owns it, so only its access list needs to change.
func windowsStorageAccess(handle windows.Handle, directory bool) (reason string, owned bool, err error) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return "", false, err
	}
	security, err := windows.GetSecurityInfo(handle, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		return "", false, err
	}
	defer runtime.KeepAlive(security)
	defer runtime.KeepAlive(user)
	owner, _, err := security.Owner()
	if err != nil {
		return "", false, err
	}
	if owner == nil || !owner.Equals(user.User.Sid) {
		return "profile storage must belong to the current user", false, nil
	}
	dacl, _, err := security.DACL()
	if err != nil || dacl == nil || !security.IsValid() {
		return "profile storage requires a private access list", true, nil
	}
	control, _, err := security.Control()
	if err != nil {
		return "", false, err
	}
	if directory && control&windows.SE_DACL_PROTECTED == 0 {
		return "data directory must exclude inherited access", true, nil
	}
	userAccess := false
	for index := uint32(0); index < uint32(dacl.AceCount); index++ {
		var ace *windows.ACCESS_ALLOWED_ACE
		if err := windows.GetAce(dacl, index, &ace); err != nil {
			return "", false, err
		}
		sidOffset := unsafe.Offsetof(ace.SidStart)
		if ace.Header.AceType != windows.ACCESS_ALLOWED_ACE_TYPE || uintptr(ace.Header.AceSize) < sidOffset+8 {
			return "profile storage has an unsupported access entry", true, nil
		}
		sid := (*windows.SID)(unsafe.Pointer(&ace.SidStart))
		if uintptr(sid.Len()) > uintptr(ace.Header.AceSize)-sidOffset || !sid.IsValid() {
			return "profile storage has an invalid access entry", true, nil
		}
		current := sid.Equals(user.User.Sid)
		if !current && !sid.IsWellKnown(windows.WinLocalSystemSid) {
			return "profile storage permissions must exclude other users", true, nil
		}
		flags := ace.Header.AceFlags
		fullAccess := uint32(ace.Mask)&fileAllAccess == fileAllAccess || uint32(ace.Mask)&windows.GENERIC_ALL != 0
		inheritable := flags&(windows.OBJECT_INHERIT_ACE|windows.CONTAINER_INHERIT_ACE) ==
			windows.OBJECT_INHERIT_ACE|windows.CONTAINER_INHERIT_ACE && flags&windows.NO_PROPAGATE_INHERIT_ACE == 0
		userAccess = userAccess || current && fullAccess && flags&windows.INHERIT_ONLY_ACE == 0 && (!directory || inheritable)
	}
	if !userAccess {
		if directory {
			return "data directory must grant inheritable current-user access", true, nil
		}
		return "profile storage must grant full current-user access", true, nil
	}
	return "", true, nil
}

func windowsName(path string) (*uint16, error) {
	if filepath.IsAbs(path) && !strings.HasPrefix(path, `\\?\`) && !strings.HasPrefix(path, `\\.\`) {
		if strings.HasPrefix(path, `\\`) {
			path = `\\?\UNC\` + strings.TrimPrefix(path, `\\`)
		} else {
			path = `\\?\` + path
		}
	}
	return windows.UTF16PtrFromString(path)
}
