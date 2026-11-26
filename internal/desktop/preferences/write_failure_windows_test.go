package preferences

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/sys/windows"
)

func TestWriteFailureWindowsReasons(t *testing.T) {
	for _, test := range []struct {
		err    error
		reason string
	}{
		{windows.ERROR_ACCESS_DENIED, "permission_denied"},
		{windows.ERROR_DISK_FULL, "no_space"},
		{windows.ERROR_HANDLE_DISK_FULL, "no_space"},
		{windows.ERROR_SHARING_VIOLATION, "file_locked"},
		{windows.ERROR_LOCK_VIOLATION, "file_locked"},
	} {
		err := &writeFailure{stage: "replace", err: fmt.Errorf("wrapped: %w", test.err)}
		if stage, reason := WriteFailure(err); stage != "replace" || reason != test.reason {
			t.Errorf("Windows reason for %v = %q/%q", test.err, stage, reason)
		}
	}
}

func TestWriteSnapshotWindowsShareLockedDestination(t *testing.T) {
	s := Open(filepath.Join(t.TempDir(), "preferences.json"))
	if err := s.Save(); err != nil {
		t.Fatal(err)
	}
	original, err := os.ReadFile(s.Path)
	if err != nil {
		t.Fatal(err)
	}
	path, err := windows.UTF16PtrFromString(s.Path)
	if err != nil {
		t.Fatal(err)
	}
	handle, err := windows.CreateFile(path, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	s.Data.ToastSeconds = 10
	err = s.Save()
	if closeErr := windows.CloseHandle(handle); closeErr != nil {
		t.Fatal(closeErr)
	}
	stage, reason := WriteFailure(err)
	// MoveFileEx can also return access denied when replacing a destination
	// opened without FILE_SHARE_DELETE. Report the error Windows returned.
	matched := errors.Is(err, windows.ERROR_ACCESS_DENIED) && reason == "permission_denied" ||
		errors.Is(err, windows.ERROR_SHARING_VIOLATION) && reason == "file_locked"
	if !matched || stage != "replace" || s.Data.ToastSeconds != 10 || !s.Readable {
		t.Fatalf("share-locked save = %v, %q/%q, data %+v", err, stage, reason, s.Data)
	}
	got, err := os.ReadFile(s.Path)
	if err != nil || string(got) != string(original) {
		t.Fatalf("failed replacement changed the saved settings: %q, %v", got, err)
	}
	temporary, err := filepath.Glob(filepath.Join(filepath.Dir(s.Path), ".preferences-*"))
	if err != nil || len(temporary) != 0 {
		t.Fatalf("failed replacement retained a temporary file: %v, %v", temporary, err)
	}
	if err := s.Save(); err != nil {
		t.Fatalf("save after releasing the handle: %v", err)
	}
}
