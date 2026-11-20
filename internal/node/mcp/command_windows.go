package mcp

import (
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

type commandGroup struct{ handle windows.Handle }

type jobBasicAccountingInformation struct {
	TotalUserTime             int64
	TotalKernelTime           int64
	ThisPeriodTotalUserTime   int64
	ThisPeriodTotalKernelTime int64
	TotalPageFaultCount       uint32
	TotalProcesses            uint32
	ActiveProcesses           uint32
	TotalTerminatedProcesses  uint32
}

func configureCommand(command *exec.Cmd) error {
	attributes := &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.CREATE_NO_WINDOW}
	switch strings.ToLower(filepath.Ext(command.Path)) {
	case ".bat", ".cmd":
		// Npm installs Windows command shims as batch files. CreateProcess
		// needs the system interpreter for these files, with literal arguments.
		words := append([]string{command.Path}, command.Args[1:]...)
		for index, word := range words {
			if strings.ContainsAny(word, "\"%\r\n\x00") {
				return invalid("MCP batch commands cannot contain double quotes, percent signs or line breaks in their path or arguments")
			}
			// Keep trailing backslashes literal when a shim forwards %* to an executable.
			trailing := word[len(strings.TrimRight(word, `\`)):]
			words[index] = `"` + word + trailing + `"`
		}
		system, err := windows.GetSystemDirectory()
		if err != nil {
			return err
		}
		command.Path = filepath.Join(system, "cmd.exe")
		attributes.CmdLine = `cmd.exe /d /s /v:off /c "` + strings.Join(words, " ") + `"`
	}
	command.SysProcAttr = attributes
	return nil
}

func retainGroup(command *exec.Cmd) (*commandGroup, error) {
	handle, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	limits := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limits.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(handle, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limits)), uint32(unsafe.Sizeof(limits))); err != nil {
		_ = windows.CloseHandle(handle)
		return nil, err
	}
	child, err := windows.OpenProcess(windows.PROCESS_SET_QUOTA|windows.PROCESS_TERMINATE, false, uint32(command.Process.Pid))
	if err == nil {
		err = windows.AssignProcessToJobObject(handle, child)
		_ = windows.CloseHandle(child)
	}
	if err != nil {
		_ = windows.CloseHandle(handle)
		return nil, err
	}
	return &commandGroup{handle}, nil
}

func (group *commandGroup) terminate() error { return windows.TerminateJobObject(group.handle, 1) }
func (group *commandGroup) kill() error      { return windows.TerminateJobObject(group.handle, 1) }
func (group *commandGroup) settled() bool {
	var accounting jobBasicAccountingInformation
	err := windows.QueryInformationJobObject(group.handle, windows.JobObjectBasicAccountingInformation, uintptr(unsafe.Pointer(&accounting)), uint32(unsafe.Sizeof(accounting)), nil)
	return err == nil && accounting.ActiveProcesses == 0
}
func (group *commandGroup) close() error { return windows.CloseHandle(group.handle) }
