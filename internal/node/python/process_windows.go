package python

import (
	"errors"
	"os"
	"os/exec"
	"runtime"
	"slices"
	"strings"
	"unicode/utf16"
	"unsafe"

	"golang.org/x/sys/windows"
)

type processGroup struct{ handle windows.Handle }

func startProcess(command *exec.Cmd) (_ *processGroup, failure error) {
	application, err := windows.UTF16PtrFromString(command.Path)
	if err != nil {
		return nil, err
	}
	arguments := make([]string, len(command.Args))
	for index, value := range command.Args {
		arguments[index] = windows.EscapeArg(value)
	}
	line, err := windows.UTF16PtrFromString(strings.Join(arguments, " "))
	if err != nil {
		return nil, err
	}
	directory, err := windows.UTF16PtrFromString(command.Dir)
	if err != nil {
		return nil, err
	}
	environment := slices.Clone(command.Env)
	slices.SortFunc(environment, func(left, right string) int { return strings.Compare(strings.ToUpper(left), strings.ToUpper(right)) })
	block := utf16.Encode([]rune(strings.Join(environment, "\x00") + "\x00\x00"))
	handle, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	defer func() {
		if failure != nil {
			_ = windows.CloseHandle(handle)
		}
	}()
	limits := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limits.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(handle, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limits)), uint32(unsafe.Sizeof(limits))); err != nil {
		return nil, err
	}
	streams := []any{command.Stdin, command.Stdout, command.Stderr}
	standard := make([]windows.Handle, 0, len(streams))
	inherited := make([]windows.Handle, 0, len(streams))
	for _, stream := range streams {
		file, ok := stream.(*os.File)
		if !ok {
			return nil, errors.New("Python preparation requires owned process streams")
		}
		owned := windows.Handle(file.Fd())
		if err := windows.SetHandleInformation(owned, windows.HANDLE_FLAG_INHERIT, windows.HANDLE_FLAG_INHERIT); err != nil {
			return nil, err
		}
		standard = append(standard, owned)
		if !slices.Contains(inherited, owned) {
			inherited = append(inherited, owned)
		}
	}
	attributes, err := windows.NewProcThreadAttributeList(1)
	if err != nil {
		return nil, err
	}
	defer attributes.Delete()
	if err := attributes.Update(windows.PROC_THREAD_ATTRIBUTE_HANDLE_LIST, unsafe.Pointer(&inherited[0]), uintptr(len(inherited))*unsafe.Sizeof(inherited[0])); err != nil {
		return nil, err
	}
	startup := windows.StartupInfoEx{
		StartupInfo: windows.StartupInfo{
			Cb: uint32(unsafe.Sizeof(windows.StartupInfoEx{})), Flags: windows.STARTF_USESTDHANDLES | windows.STARTF_USESHOWWINDOW,
			ShowWindow: windows.SW_HIDE, StdInput: standard[0], StdOutput: standard[1], StdErr: standard[2],
		},
		ProcThreadAttributeList: attributes.List(),
	}
	var information windows.ProcessInformation
	flags := uint32(windows.CREATE_SUSPENDED | windows.CREATE_UNICODE_ENVIRONMENT | windows.EXTENDED_STARTUPINFO_PRESENT | windows.CREATE_NEW_PROCESS_GROUP | windows.CREATE_NO_WINDOW)
	if err := windows.CreateProcess(application, line, nil, nil, true, flags, &block[0], directory, &startup.StartupInfo, &information); err != nil {
		return nil, err
	}
	defer windows.CloseHandle(information.Process)
	defer windows.CloseHandle(information.Thread)
	if err := windows.AssignProcessToJobObject(handle, information.Process); err != nil {
		_ = windows.TerminateProcess(information.Process, 1)
		return nil, err
	}
	process, err := os.FindProcess(int(information.ProcessId))
	if err != nil {
		_ = windows.TerminateJobObject(handle, 1)
		return nil, err
	}
	command.Process = process
	if _, err := windows.ResumeThread(information.Thread); err != nil {
		_ = windows.TerminateJobObject(handle, 1)
		_ = process.Release()
		command.Process = nil
		return nil, err
	}
	runtime.KeepAlive(block)
	return &processGroup{handle}, nil
}

func (group *processGroup) kill() error  { return windows.TerminateJobObject(group.handle, 1) }
func (group *processGroup) close() error { return windows.CloseHandle(group.handle) }
