package logging

import (
	"os"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func assertLogPrivacy(t *testing.T, path string) {
	t.Helper()
	if info, err := os.Lstat(path); err != nil || !info.Mode().IsRegular() {
		t.Fatalf("log is not a regular file: %v, %v", info, err)
	}
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		t.Fatal(err)
	}
	security, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	owner, _, err := security.Owner()
	if err != nil || owner == nil || !owner.Equals(user.User.Sid) {
		t.Fatalf("log owner differs from the current user: %v", err)
	}
	dacl, _, err := security.DACL()
	if err != nil || dacl == nil {
		t.Fatalf("log has no private access list: %v", err)
	}
	full := false
	for index := uint32(0); index < uint32(dacl.AceCount); index++ {
		var ace *windows.ACCESS_ALLOWED_ACE
		if err := windows.GetAce(dacl, index, &ace); err != nil {
			t.Fatal(err)
		}
		if ace.Header.AceType != windows.ACCESS_ALLOWED_ACE_TYPE {
			t.Fatalf("log has an unexpected access entry: %d", ace.Header.AceType)
		}
		sid := (*windows.SID)(unsafe.Pointer(&ace.SidStart))
		current := sid.Equals(user.User.Sid)
		if !current && !sid.IsWellKnown(windows.WinLocalSystemSid) {
			t.Fatal("log permissions grant another user access")
		}
		access := uint32(ace.Mask)
		fullAccess := access&0x001f01ff == 0x001f01ff || access&windows.GENERIC_ALL != 0
		full = full || current && fullAccess && ace.Header.AceFlags&windows.INHERIT_ONLY_ACE == 0
	}
	if !full {
		t.Fatal("log does not grant the current user full access")
	}
}
