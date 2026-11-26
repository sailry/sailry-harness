package updater

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/identity"
	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

func systemVersion() (string, error) {
	if runtime.GOARCH != "amd64" {
		return "", failure("updates_platform", "no Windows update package is published for this architecture")
	}
	version := windows.RtlGetVersion()
	return fmt.Sprintf("%d.%d.%d", version.MajorVersion, version.MinorVersion, version.BuildNumber), nil
}

func installedBundle(product identity.DesktopPackage) (string, error) {
	path, err := os.Executable()
	if err != nil {
		return "", err
	}
	path, err = filepath.EvalSymlinks(path)
	if err != nil {
		return "", err
	}
	if !strings.EqualFold(filepath.Base(path), product.Name+".exe") {
		return "", failure("updates_bundle_required", "updates require a packaged Windows application")
	}
	root := filepath.Dir(path)
	metadata, err := readLimited(filepath.Join(root, "build.json"), 1024*1024)
	var packaged build
	if err != nil || json.Unmarshal(metadata, &packaged) != nil || packaged.Version != 1 || packaged.Target != Target() || packaged.ApplicationVersion == "" {
		return "", failure("updates_bundle_required", "the running Windows executable has no matching package metadata")
	}
	return root, nil
}

func exchangeBundles(string, string) error {
	return failure("updates_platform", "Windows replaces application files after the process exits")
}

type windowsInstall struct {
	Installed, Backup, Receipt, Bundle, Executable, Version, ProductID string
	Arguments                                                          []string
}

func (s *Service) installWindows(update *staged, config *Config, installed, root string) (bool, error) {
	// The child lives in a per-user cache and must run with that user's
	// permissions even when the desktop was manually launched as administrator.
	var elevated, size uint32
	if err := windows.GetTokenInformation(windows.GetCurrentProcessToken(), windows.TokenElevation, (*byte)(unsafe.Pointer(&elevated)), uint32(unsafe.Sizeof(elevated)), &size); err != nil {
		return false, ioFailure(err)
	}
	if elevated != 0 {
		return false, failure("updates_windows_elevated", "restart without administrator privileges to install this per-user update")
	}
	return s.handoffWindows(update, config, installed, root)
}

func (s *Service) handoffWindows(update *staged, config *Config, installed, root string) (bool, error) {
	release := update.selection.Release
	backup, err := os.MkdirTemp(filepath.Dir(installed), ".sailry-update-backup-")
	if err != nil {
		return false, ioFailure(err)
	}
	handedOff := false
	defer func() {
		if !handedOff {
			_ = os.RemoveAll(backup)
		}
	}()
	incoming := filepath.Join(backup, "incoming")
	if err := os.Mkdir(incoming, 0o700); err != nil {
		return false, ioFailure(err)
	}
	bundle, err := config.extract(update.archive, release, incoming)
	if err != nil {
		return false, err
	}
	if err := os.Mkdir(filepath.Join(backup, release.Bundle), 0o700); err != nil {
		return false, ioFailure(err)
	}
	// Copy the verified tool outside both trees. Windows keeps a running EXE
	// mapped; the next start may safely delete the entire transaction.
	helper := filepath.Join(root, windowsHelper)
	tmp, err := os.MkdirTemp(root, "helper-")
	if err != nil {
		return false, ioFailure(err)
	}
	defer os.RemoveAll(tmp)
	name := filepath.Join(tmp, windowsHelper)
	if err := copyTree(filepath.Join(bundle, windowsHelper), name); err != nil {
		return false, err
	}
	if err := os.Rename(name, helper); err != nil {
		return false, ioFailure(err)
	}
	plan := windowsInstall{Installed: installed, Backup: backup, Receipt: filepath.Join(root, "receipt.json"), Bundle: release.Bundle,
		Executable: release.Executable, Version: release.Version, ProductID: config.packageIdentity().ID, Arguments: os.Args[1:]}
	if err := s.options.Stop(); err != nil {
		return true, failure("updates_shutdown_failed", err.Error())
	}
	if err := startWindowsHelper(helper, plan); err != nil {
		return true, failure("updates_install_failed", err.Error())
	}
	handedOff = true
	return true, nil
}

// Inherit a synchronize-only handle to this exact process. A PID lookup in
// the child would race exit and reuse. The helper inherits the working
// directory as well, preserving any relative paths in the original arguments.
func startWindowsHelper(helper string, plan windowsInstall) error {
	body, err := json.Marshal(plan)
	if err != nil {
		return err
	}
	planPath := filepath.Join(plan.Backup, "install.json")
	if err := os.WriteFile(planPath, body, 0o600); err != nil {
		return err
	}
	handle, err := windows.OpenProcess(windows.SYNCHRONIZE, true, uint32(os.Getpid()))
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	cmd := exec.Command(helper, strconv.FormatUint(uint64(handle), 10), planPath)
	cmd.SysProcAttr = &syscall.SysProcAttr{AdditionalInheritedHandles: []syscall.Handle{syscall.Handle(handle)}}
	if err := cmd.Start(); err != nil {
		return err
	}
	_ = cmd.Process.Release()
	return nil
}

// FinishWindowsInstall runs in the signed package's standalone updater.
func FinishWindowsInstall(args []string) error {
	if len(args) != 2 {
		return fmt.Errorf("expected a parent process handle and an installation plan")
	}
	h, err := strconv.ParseUint(args[0], 10, strconv.IntSize)
	if err != nil {
		return err
	}
	handle := windows.Handle(h)
	defer windows.CloseHandle(handle)
	body, err := os.ReadFile(args[1])
	if err != nil {
		return err
	}
	var plan windowsInstall
	if err := json.Unmarshal(body, &plan); err != nil {
		return err
	}
	if !filepath.IsAbs(plan.Installed) || filepath.Dir(plan.Backup) != filepath.Dir(plan.Installed) ||
		!strings.HasPrefix(filepath.Base(plan.Backup), ".sailry-update-backup-") ||
		filepath.Join(plan.Backup, "install.json") != args[1] || !packageComponent(plan.Bundle) ||
		!strings.EqualFold(plan.Executable, plan.Bundle+".exe") {
		return fmt.Errorf("invalid Windows installation plan")
	}
	event, err := windows.WaitForSingleObject(handle, windows.INFINITE)
	if err != nil || event != windows.WAIT_OBJECT_0 {
		return fmt.Errorf("waiting for the desktop process: result %d, error %v", event, err)
	}
	original := filepath.Join(plan.Backup, plan.Bundle)
	// A crash or power loss after the first file replacement must leave an
	// actionable receipt in the same place checked by the next desktop.
	message := "Windows update interrupted; retained files are at " + original
	uncertain := true
	pending := receipt{Version: 1, Result: "failure", Message: &message, Uncertain: &uncertain, Backup: &plan.Backup}
	executable := filepath.Join(plan.Installed, plan.Executable)
	if err := writeReceipt(plan.Receipt, pending); err != nil {
		return errors.Join(err, relaunch(executable, plan.Arguments))
	}
	err = applyWindowsFiles(filepath.Join(plan.Backup, "incoming", plan.Bundle), plan.Installed, original)
	result := receipt{Version: 1, Result: "success", ApplicationVersion: &plan.Version, Backup: &plan.Backup}
	if err != nil {
		message = fmt.Sprintf("%s; installation backup retained at %s", err, original)
		var f *Failure
		uncertain = errors.As(err, &f) && f.Key == "updates_install_uncertain"
		result = pending
	} else {
		refreshWindowsRegistration(plan)
	}
	recordErr := replaceWindowsReceipt(plan.Receipt, result)
	launchErr := relaunch(executable, plan.Arguments)
	if launchErr != nil && err == nil && recordErr == nil {
		installed := true
		message = fmt.Sprintf("%s; backup retained at %s", launchErr, original)
		result.Result, result.Message, result.Installed = "failure", &message, &installed
		recordErr = replaceWindowsReceipt(plan.Receipt, result)
	}
	return errors.Join(err, recordErr, launchErr)
}

func replaceWindowsReceipt(path string, result receipt) error {
	dir, err := os.MkdirTemp(filepath.Dir(path), "result-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(dir)
	temp := filepath.Join(dir, "receipt.json")
	if err := writeReceipt(temp, result); err != nil {
		return err
	}
	return os.Rename(temp, path)
}

// Update an existing per-user NSIS registration only for its exact install
// directory. Portable copies do not register themselves.
func refreshWindowsRegistration(plan windowsInstall) {
	if !packageComponent(plan.ProductID) {
		return
	}
	key, err := registry.OpenKey(registry.CURRENT_USER, `Software\Microsoft\Windows\CurrentVersion\Uninstall\`+plan.ProductID, registry.QUERY_VALUE|registry.SET_VALUE)
	if err != nil {
		return
	}
	defer key.Close()
	dir, _, err := key.GetStringValue("InstallLocation")
	if err == nil && strings.EqualFold(filepath.Clean(dir), filepath.Clean(plan.Installed)) {
		_ = key.SetStringValue("DisplayVersion", plan.Version)
	}
}
