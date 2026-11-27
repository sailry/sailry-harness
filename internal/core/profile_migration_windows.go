package core

import (
	"errors"
	"os"
	"path/filepath"
	"unsafe"

	"golang.org/x/sys/windows"
)

const windowsMigrationLockName = "migration-source.lock"

// AcquireWindowsMigrationLock locks an existing node.lock through a second name
// in the exclusively owned sibling journal. An open handle through the profile
// name would keep NTFS from renaming the directory. The original name is never
// removed. If existingOnly is set, a missing journal link is a no-op.
func (journal *Profile) AcquireWindowsMigrationLock(path string, existingOnly bool) (_ func() error, failure error) {
	if !filepath.IsAbs(path) || journal == nil || journal.lock == nil {
		return nil, invalidProfile(path, "legacy migration requires an absolute path and an owned journal")
	}
	alias := filepath.Join(journal.Path, windowsMigrationLockName)
	_, err := os.Lstat(alias)
	missing := errors.Is(err, os.ErrNotExist)
	if missing && existingOnly {
		return nil, nil
	}
	if err != nil && !missing {
		return nil, err
	}
	if _, err := os.Lstat(path); err != nil {
		return nil, err
	}
	if err := PrivateDirectory(path); err != nil {
		return nil, err
	}
	primary := filepath.Join(path, "node.lock")
	var access uint32
	wantLinks := uint32(2)
	if missing {
		// Validate with the normal single-link policy before making the exception.
		if err := validateWindowsStorage(primary, false); err != nil {
			return nil, err
		}
		access = windows.SYNCHRONIZE | windows.FILE_WRITE_ATTRIBUTES
		wantLinks = 1
	}
	source, original, err := openWindowsStorageHandle(primary, false, access)
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(source)
	if original.NumberOfLinks != wantLinks {
		return nil, invalidProfile(primary, "legacy migration permits only node.lock and its journal link")
	}
	var filesystem [32]uint16
	if err := windows.GetVolumeInformationByHandle(source, nil, 0, nil, nil, nil, &filesystem[0], uint32(len(filesystem))); err != nil {
		return nil, err
	}
	if windows.UTF16ToString(filesystem[:]) != "NTFS" {
		return nil, invalidProfile(path, "legacy migration requires NTFS for the profile lock")
	}
	if err := validateExistingWindowsFile(filepath.Join(path, "link.key")); err != nil {
		return nil, err
	}
	var lock *os.File
	var removeOnFailure, locked bool
	defer func() {
		if failure == nil {
			return
		}
		// Only a newly created link may be removed without exclusive locking.
		if removeOnFailure {
			failure = errors.Join(failure, unlinkWindowsMigrationLock(alias, original))
		}
		if lock != nil {
			if locked {
				failure = errors.Join(failure, unlockProfile(lock))
			}
			failure = errors.Join(failure, lock.Close())
		}
	}()
	if missing {
		if err := journal.linkWindowsMigrationLock(source); err != nil {
			return nil, err
		}
		removeOnFailure = true
	}
	handle, linked, err := openWindowsStorageHandle(alias, false, windows.GENERIC_READ|windows.GENERIC_WRITE)
	if err != nil {
		return nil, err
	}
	lock = os.NewFile(uintptr(handle), alias)
	if !sameWindowsStorage(original, linked) {
		return nil, invalidProfile(alias, "journal lock is not the profile's node.lock")
	}
	if linked.NumberOfLinks != 2 {
		return nil, invalidProfile(primary, "legacy migration permits only node.lock and its journal link")
	}
	if reason, _, err := windowsStorageAccess(handle, false); err != nil {
		return nil, err
	} else if reason != "" {
		return nil, invalidProfile(primary, reason)
	}
	if err := lockWindowsProfile(path, handle); err != nil {
		return nil, err
	}
	locked = true
	if err := checkWindowsMigrationLock(primary, linked, 2); err != nil {
		return nil, err
	}
	// The lock survives unlinking the name it was opened through. Retire that
	// name before touching the profile, so a crash during migration cannot leave
	// a hard link that prevents older Node versions from locking node.lock.
	removeOnFailure = false
	if err := unlinkWindowsMigrationLock(alias, linked); err != nil {
		return nil, err
	}
	if err := checkWindowsMigrationLock(primary, linked, 1); err != nil {
		return nil, err
	}
	if err := prepareNodeProfileDirectories(path); err != nil {
		return nil, err
	}
	return func() error { return errors.Join(unlockProfile(lock), lock.Close()) }, nil
}

func checkWindowsMigrationLock(primary string, linked windows.ByHandleFileInformation, links uint32) error {
	current, info, err := openWindowsStorageHandle(primary, false, 0)
	if err != nil {
		return err
	}
	_ = windows.CloseHandle(current)
	if info.NumberOfLinks != links || !sameWindowsStorage(linked, info) {
		return invalidProfile(primary, "profile lock changed during migration lock acquisition")
	}
	return nil
}

func (journal *Profile) linkWindowsMigrationLock(source windows.Handle) error {
	directory, _, err := openWindowsStorage(journal.Path, true, windows.FILE_TRAVERSE)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(directory)
	name, err := windows.UTF16FromString(windowsMigrationLockName)
	if err != nil {
		return err
	}
	link := struct {
		ReplaceIfExists byte
		RootDirectory   windows.Handle
		FileNameLength  uint32
		FileName        [len(windowsMigrationLockName) + 1]uint16
	}{RootDirectory: directory, FileNameLength: uint32(2 * (len(name) - 1))}
	copy(link.FileName[:], name)
	// FileLinkInformation (11) is the original no-replace layout, without the
	// flags in FileLinkInformationEx (72).
	const fileLinkInformation = 11
	err = windows.NtSetInformationFile(source, new(windows.IO_STATUS_BLOCK), (*byte)(unsafe.Pointer(&link)), uint32(unsafe.Sizeof(link)), fileLinkInformation)
	return fileWindowsStatus(err)
}

func unlinkWindowsMigrationLock(alias string, expected windows.ByHandleFileInformation) error {
	// Use a separate handle for disposition. Closing it removes the selected
	// link without closing the LockFileEx handle or touching node.lock.
	handle, info, err := openWindowsStorageHandle(alias, false, windows.DELETE)
	if err != nil {
		return err
	}
	if info.NumberOfLinks < 2 || !sameWindowsStorage(expected, info) {
		_ = windows.CloseHandle(handle)
		return invalidProfile(alias, "refusing to remove an unrecognized or sole lock link")
	}
	flags := uint32(windows.FILE_DISPOSITION_DELETE | windows.FILE_DISPOSITION_POSIX_SEMANTICS)
	err = windows.SetFileInformationByHandle(handle, windows.FileDispositionInfoEx, (*byte)(unsafe.Pointer(&flags)), uint32(unsafe.Sizeof(flags)))
	if err := errors.Join(err, windows.CloseHandle(handle)); err != nil {
		return &os.PathError{Op: "unlink legacy migration lock", Path: alias, Err: err}
	}
	if _, err := os.Lstat(alias); !errors.Is(err, os.ErrNotExist) {
		if err != nil {
			return err
		}
		return invalidProfile(alias, "legacy migration lock link did not disappear")
	}
	return nil
}
