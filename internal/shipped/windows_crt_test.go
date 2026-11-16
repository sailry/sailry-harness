package shipped

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestWindowsCRTSeal(t *testing.T) {
	directory := t.TempDir()
	crt := WindowsCRT{Version: 1, Target: "windows/amd64", Toolset: "fixture", Files: []string{"msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "windows-crt-Redist.txt", "windows-crt-ThirdPartyNotices.txt"}}
	manifest, err := json.Marshal(crt)
	if err != nil {
		t.Fatal(err)
	}
	seal := &Seal{Version: 1, Target: "windows/amd64"}
	for _, name := range append([]string{WindowsCRTManifest}, crt.Files...) {
		body := []byte(name)
		if name == WindowsCRTManifest {
			body = manifest
		}
		if err := os.WriteFile(filepath.Join(directory, name), body, 0o600); err != nil {
			t.Fatal(err)
		}
		record, err := Record(directory, name)
		if err != nil {
			t.Fatal(err)
		}
		seal.Files = append(seal.Files, record)
	}
	if err := seal.CheckWindowsCRT(directory); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{WindowsCRTManifest, "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"} {
		original, err := os.ReadFile(filepath.Join(directory, name))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, name), []byte("changed"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := seal.CheckWindowsCRT(directory); err == nil {
			t.Errorf("accepted changed %s", name)
		}
		if err := os.WriteFile(filepath.Join(directory, name), original, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	seal.Files = seal.Files[:len(seal.Files)-1]
	if err := seal.CheckWindowsCRT(directory); err == nil {
		t.Fatal("accepted a runtime file omitted from the seal")
	}
	if err := (&Seal{Target: "linux/amd64"}).CheckWindowsCRT(t.TempDir()); err != nil {
		t.Fatalf("required Microsoft runtime on Linux: %v", err)
	}
}

func TestWindowsCRTManifest(t *testing.T) {
	for name, body := range map[string]string{
		"invalid JSON": "{",
		"unsupported":  `{"version":2,"files":[]}`,
		"missing CRT":  `{"version":1,"target":"windows/amd64","toolset":"fixture","files":[]}`,
		"path":         `{"version":1,"target":"windows/amd64","toolset":"fixture","files":["../msvcp140.dll"]}`,
		"Windows path": `{"version":1,"target":"windows/amd64","toolset":"fixture","files":["..\\msvcp140.dll"]}`,
		"duplicate":    `{"version":1,"target":"windows/amd64","toolset":"fixture","files":["msvcp140.dll","MSVCP140.dll"]}`,
	} {
		t.Run(name, func(t *testing.T) {
			directory := t.TempDir()
			if err := os.WriteFile(filepath.Join(directory, WindowsCRTManifest), []byte(body), 0o600); err != nil {
				t.Fatal(err)
			}
			if _, err := ReadWindowsCRT(directory); err == nil {
				t.Fatal("accepted an invalid runtime manifest")
			}
		})
	}
}
