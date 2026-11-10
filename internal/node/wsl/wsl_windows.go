//go:build windows

package wsl

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"syscall"
	"unsafe"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

// registrations is the per-user key whose GUID subkeys are the registered
// distributions.
const registrations = `Software\Microsoft\Windows\CurrentVersion\Lxss`

func discover(ctx context.Context, environment []string) (source, error) {
	executable := locate(environment)
	if executable == "" {
		return source{}, nil
	}
	names, err := list(ctx, executable, environment)
	if err != nil {
		return source{}, err
	}
	distributions, err := versions(names)
	if err != nil {
		return source{}, err
	}
	return source{executable: executable, distributions: distributions}, nil
}

// locate prefers the system launcher and otherwise searches the Node's
// captured PATH. Only an absolute existing file is accepted.
func locate(environment []string) string {
	candidates := []string{}
	if root := os.Getenv("SystemRoot"); root != "" {
		candidates = append(candidates, filepath.Join(root, "System32", "wsl.exe"))
	}
	for _, entry := range environment {
		name, value, _ := strings.Cut(entry, "=")
		if !strings.EqualFold(name, "PATH") {
			continue
		}
		for _, directory := range filepath.SplitList(value) {
			if directory != "" {
				candidates = append(candidates, filepath.Join(directory, "wsl.exe"))
			}
		}
	}
	for _, candidate := range candidates {
		if info, err := os.Stat(candidate); err == nil && info.Mode().IsRegular() {
			if filepath.IsAbs(candidate) {
				return candidate
			}
			return ""
		}
	}
	return ""
}

// list asks the launcher for names only when a distribution is registered,
// so a machine without WSL distributions never starts wsl.exe.
func list(ctx context.Context, executable string, environment []string) ([]string, error) {
	root, err := registry.OpenKey(registry.CURRENT_USER, registrations, registry.READ)
	found, err := registered(root, err)
	if err != nil || !found {
		return nil, err
	}
	body, err := query(ctx, executable, []string{"--list", "--quiet"}, environment, discoveryDeadline)
	if err != nil {
		return nil, err
	}
	return decodeDistributions(body)
}

// registered takes the result of opening the registrations key and closes
// the key. A missing key means no distribution; other errors are reported.
func registered(root registry.Key, err error) (bool, error) {
	if errors.Is(err, windows.ERROR_FILE_NOT_FOUND) || errors.Is(err, windows.ERROR_PATH_NOT_FOUND) {
		return false, nil
	}
	if err != nil {
		return false, registryFailure("registration read failed", err)
	}
	defer root.Close()
	keys, err := root.ReadSubKeyNames(-1)
	if err != nil {
		return false, registryFailure("registration enumeration failed", err)
	}
	return slices.ContainsFunc(keys, distributionKey), nil
}

func versions(names []string) ([]protocol.WslDistribution, error) {
	if len(names) == 0 {
		return nil, nil
	}
	root, err := registry.OpenKey(registry.CURRENT_USER, registrations, registry.READ)
	if err != nil {
		return nil, registryFailure("distribution registration read failed", err)
	}
	defer root.Close()
	return metadata(root, names)
}

// metadata reads the WSL version of each listed name. A listed name without
// a registration means the registry changed after the list was read.
func metadata(root registry.Key, names []string) ([]protocol.WslDistribution, error) {
	keys, err := root.ReadSubKeyNames(-1)
	if err != nil {
		return nil, registryFailure("distribution registration read failed", err)
	}
	found := map[string]uint32{}
	for _, name := range keys {
		if !distributionKey(name) {
			continue
		}
		if err := readVersion(root, name, names, found); err != nil {
			return nil, registryFailure("distribution registration read failed", err)
		}
	}
	distributions := make([]protocol.WslDistribution, 0, len(names))
	for _, name := range names {
		version, ok := found[name]
		if !ok {
			return nil, discoveryFailure("distribution registration changed; refresh the distribution list")
		}
		distributions = append(distributions, protocol.WslDistribution{Name: name, Version: version})
	}
	return distributions, nil
}

func readVersion(root registry.Key, name string, names []string, found map[string]uint32) error {
	key, err := registry.OpenKey(root, name, registry.QUERY_VALUE)
	if err != nil {
		return err
	}
	defer key.Close()
	distribution, _, err := key.GetStringValue("DistributionName")
	if err != nil || !slices.Contains(names, distribution) {
		return err
	}
	version, kind, err := key.GetIntegerValue("Version")
	if err != nil {
		return err
	}
	if kind != registry.DWORD {
		return registry.ErrUnexpectedType
	}
	found[distribution] = uint32(version)
	return nil
}

// distributionKey matches a braced GUID subkey; other subkeys are metadata.
func distributionKey(name string) bool {
	return len(name) == 38 && uuid.Validate(name) == nil
}

func registryFailure(message string, err error) error {
	code := uint32(0x80004005)
	var number syscall.Errno
	if errors.Is(err, registry.ErrUnexpectedType) {
		number = windows.ERROR_INVALID_DATA
	}
	if number != 0 || errors.As(err, &number) {
		code = 0x80070000 | uint32(number)&0xffff
	}
	return discoveryFailure(fmt.Sprintf("%s (HRESULT 0x%08X)", message, code))
}

// start launches the command without a console window inside a job that
// ends its whole process tree on cancellation and on release.
func start(command *exec.Cmd) (func(), error) {
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	limits := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limits.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limits)), uint32(unsafe.Sizeof(limits))); err != nil {
		_ = windows.CloseHandle(job)
		return nil, err
	}
	release := func() {
		_ = windows.TerminateJobObject(job, 1)
		_ = windows.CloseHandle(job)
	}
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NO_WINDOW}
	command.Cancel = func() error { return windows.TerminateJobObject(job, 1) }
	if err := command.Start(); err != nil {
		release()
		return nil, err
	}
	child, err := windows.OpenProcess(windows.PROCESS_SET_QUOTA|windows.PROCESS_TERMINATE, false, uint32(command.Process.Pid))
	if err == nil {
		err = windows.AssignProcessToJobObject(job, child)
		_ = windows.CloseHandle(child)
	}
	if err != nil {
		_ = command.Process.Kill()
		_ = command.Wait()
		release()
		return nil, err
	}
	return release, nil
}
