package mcp

import (
	"os/exec"
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

func configureGroup(command *exec.Cmd) {
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.CREATE_NO_WINDOW}
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
