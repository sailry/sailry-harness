package node

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/files"
	"golang.org/x/sys/windows"
)

func TestWorkspaceFileKeyWindowsShortName(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	name := "long-workspace-filename.txt"
	path := filepath.Join(root, name)
	if err := os.WriteFile(path, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	long, err := windows.UTF16PtrFromString(path)
	if err != nil {
		t.Fatal(err)
	}
	n, err := windows.GetShortPathName(long, nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	short := make([]uint16, n)
	if _, err := windows.GetShortPathName(long, &short[0], n); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Base(windows.UTF16ToString(short))
	if strings.EqualFold(alias, name) {
		t.Skip("the test volume does not create short file names")
	}
	if workspaceFileKey(root, name) != workspaceFileKey(root, alias) {
		t.Fatal("short and long names reserved different files")
	}
}

func TestWorkspaceFileKeyWindowsUnavailableDirectoryAndUNC(t *testing.T) {
	root := filepath.Join(t.TempDir(), "absent")
	if files.LookupPathKey(root, "File") != files.LookupPathKey(root, "file") {
		t.Fatal("missing directory metadata split a case-insensitive file key")
	}
	if files.LookupPathKey(`\\HOST\SHARE\`, "") != files.LookupPathKey(`\\host\share\`, "") {
		t.Fatal("alternate UNC root spelling split the volume key")
	}
}
