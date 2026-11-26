package updater

import (
	"archive/zip"
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"golang.org/x/sys/windows"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"testing"
	"time"
)

// TestWindowsUpdateHandoff runs a packaged fixture EXE and the real helper
// as separate Windows processes. The running fixture holds the old executable
// mapped until the test lets it exit, then the successor acknowledges its receipt.
func TestWindowsUpdateHandoff(t *testing.T) {
	if root := os.Getenv("SAILRY_TEST_WINDOWS_HANDOFF"); root != "" {
		runWindowsUpdateFixture(t, root)
		return
	}
	root := t.TempDir()
	installed := filepath.Join(root, "Portable app")
	if err := os.Mkdir(installed, 0o700); err != nil {
		t.Fatal(err)
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	if err := copyTree(exe, filepath.Join(installed, "Sailry.exe")); err != nil {
		t.Fatal(err)
	}
	run := exec.CommandContext(t.Context(), "go", "build", "-o", filepath.Join(root, windowsHelper), "./cmd/windows-install")
	if body, err := run.CombinedOutput(); err != nil {
		t.Fatalf("build updater: %v: %s", err, body)
	}
	for name, body := range map[string]string{
		"build.json":  `{"version":1,"target":"x86_64-pc-windows-msvc","application_version":"1.0.0","files":["build.json","Sailry.exe","retired.dll"]}`,
		"retired.dll": "old", "Uninstall.exe": "fixture uninstaller", "notes.txt": "local",
	} {
		if err := os.WriteFile(filepath.Join(installed, name), []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	run = exec.CommandContext(t.Context(), filepath.Join(installed, "Sailry.exe"), "-test.run=^TestWindowsUpdateHandoff$")
	run.Dir = root
	run.Env = append(os.Environ(), "SAILRY_TEST_WINDOWS_HANDOFF="+root)
	input, err := run.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	defer input.Close()
	var output bytes.Buffer
	run.Stdout, run.Stderr = &output, &output
	if err := run.Start(); err != nil {
		t.Fatal(err)
	}
	defer run.Process.Kill()
	finished := make(chan error, 1)
	go func() { finished <- run.Wait() }()
	timeout := time.NewTimer(2 * time.Minute)
	defer timeout.Stop()
	for {
		if _, err := os.Stat(filepath.Join(root, "handed-off")); err == nil {
			break
		}
		select {
		case err := <-finished:
			t.Fatalf("desktop exited before handoff: %v: %s", err, output.String())
		case <-timeout.C:
			t.Fatal("desktop did not hand off")
		case <-time.After(25 * time.Millisecond):
		}
	}
	if _, err := os.Stat(filepath.Join(installed, "retired.dll")); err != nil {
		t.Fatalf("helper replaced files before the parent exited: %v", err)
	}
	input.Close()
	if err := <-finished; err != nil {
		t.Fatalf("old desktop: %v: %s", err, output.String())
	}
	for {
		if data, err := os.ReadFile(filepath.Join(root, "restarted")); err == nil && len(data) > 0 {
			pid, err := strconv.Atoi(string(data))
			if err != nil {
				t.Fatal(err)
			}
			// The successor holds the fixture executable mapped until it exits.
			h, err := windows.OpenProcess(windows.SYNCHRONIZE, false, uint32(pid))
			if err == nil {
				event, err := windows.WaitForSingleObject(h, 30000)
				windows.CloseHandle(h)
				if err != nil || event != windows.WAIT_OBJECT_0 {
					t.Fatalf("successor exit: %d %v", event, err)
				}
			} else if err != windows.ERROR_INVALID_PARAMETER {
				t.Fatal(err)
			}
			break
		}
		select {
		case <-timeout.C:
			t.Fatal("updated desktop did not confirm the receipt")
		case <-time.After(25 * time.Millisecond):
		}
	}
	for name, want := range map[string]string{"Uninstall.exe": "fixture uninstaller", "notes.txt": "local", "new.dll": "new"} {
		body, err := os.ReadFile(filepath.Join(installed, name))
		if err != nil || string(body) != want {
			t.Fatalf("%s = %q: %v", name, body, err)
		}
	}
	if _, err := os.Stat(filepath.Join(installed, "retired.dll")); !os.IsNotExist(err) {
		t.Fatalf("retired payload still present: %v", err)
	}
}

func runWindowsUpdateFixture(t *testing.T, root string) {
	installed := filepath.Join(root, "Portable app")
	cache := filepath.Join(root, "cache")
	version, err := os.ReadFile(filepath.Join(installed, "build.json"))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(version, []byte("1.1.0")) {
		notice, err := readReceipt(cache, installed, "1.1.0", "Sailry")
		if err != nil || notice != "updates_completed" {
			t.Fatalf("successor receipt: %q %v", notice, err)
		}
		cwd, _ := os.Getwd()
		if cwd != root {
			t.Fatalf("successor cwd = %q, want %q", cwd, root)
		}
		if err := os.WriteFile(filepath.Join(root, "restarted"), []byte(strconv.Itoa(os.Getpid())), 0o600); err != nil {
			t.Fatal(err)
		}
		return
	}
	for _, dir := range []string{cache, filepath.Join(cache, "download", "contents")} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	key := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{7}, ed25519.SeedSize))
	config := Config{Target: Target(), System: "10.0", Version: "1.0.0", Keys: []ed25519.PublicKey{key.Public().(ed25519.PublicKey)}}
	archive := filepath.Join(root, "source.zip")
	out, err := os.Create(archive)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(out)
	for name, path := range map[string]string{"Sailry.exe": filepath.Join(installed, "Sailry.exe"), windowsHelper: filepath.Join(root, windowsHelper)} {
		file, err := os.Open(path)
		if err != nil {
			t.Fatal(err)
		}
		member, err := writer.CreateHeader(&zip.FileHeader{Name: "Sailry/" + name, Method: zip.Store})
		if err != nil {
			t.Fatal(err)
		}
		_, err = io.Copy(member, file)
		file.Close()
		if err != nil {
			t.Fatal(err)
		}
	}
	for name, data := range map[string]string{"build.json": `{"version":1,"target":"x86_64-pc-windows-msvc","application_version":"1.1.0","files":["build.json","Sailry.exe","new.dll","sailry-update.exe"]}`, "new.dll": "new"} {
		file, err := writer.Create("Sailry/" + name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := io.WriteString(file, data); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := out.Close(); err != nil {
		t.Fatal(err)
	}
	name := "Sailry-" + Target() + ".zip"
	stage := filepath.Join(cache, "download")
	signed := filepath.Join(stage, name)
	if err := signedCopy(key, archive, signed); err != nil {
		t.Fatal(err)
	}
	hash, size, err := measure(signed)
	if err != nil {
		t.Fatal(err)
	}
	release := Release{Target: Target(), Name: name, Bundle: "Sailry", Executable: "Sailry.exe", Version: "1.1.0", MinimumSystem: "10.0", SHA256: hash, Size: size, URL: "https://example.test/" + name}
	payload, _ := json.Marshal(payload{Releases: []Release{release}})
	manifest, _ := json.Marshal(envelope{Version: 1, Payload: payload, Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(key, payload))})
	selection, err := config.selectRelease(manifest)
	if err != nil || selection == nil {
		t.Fatalf("selection: %v", err)
	}
	if _, err := config.extract(signed, release, filepath.Join(stage, "contents")); err != nil {
		t.Fatal(err)
	}
	service := New(Options{Version: "1.0.0", Stop: func() error { return nil }})
	defer service.Close()
	if err := config.revalidate(selection); err != nil {
		t.Fatal(err)
	}
	if err := verifyArchive(signed, release, config.Keys); err != nil {
		t.Fatal(err)
	}
	if stopped, err := service.handoffWindows(&staged{directory: stage, archive: signed, selection: selection}, &config, installed, cache); !stopped || err != nil {
		t.Fatalf("handoff = %t, %v", stopped, err)
	}
	if err := os.RemoveAll(stage); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "handed-off"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := io.Copy(io.Discard, os.Stdin); err != nil {
		t.Fatal(fmt.Errorf("waiting to quit: %w", err))
	}
}
