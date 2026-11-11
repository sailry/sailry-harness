package externalbrowser

import (
	"os"
	"os/exec"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

// processGroup is a job object that kills the browser tree when it closes,
// including when the Node exits without closing it.
type processGroup struct{ handle windows.Handle }

func configureProcess(command *exec.Cmd) {
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.CREATE_NO_WINDOW}
}

func retainProcess(process *os.Process) (*processGroup, error) {
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
	child, err := windows.OpenProcess(windows.PROCESS_SET_QUOTA|windows.PROCESS_TERMINATE, false, uint32(process.Pid))
	if err == nil {
		err = windows.AssignProcessToJobObject(handle, child)
		_ = windows.CloseHandle(child)
	}
	if err != nil {
		_ = windows.CloseHandle(handle)
		return nil, err
	}
	return &processGroup{handle}, nil
}

func (group *processGroup) kill() {
	_ = windows.TerminateJobObject(group.handle, 1)
	_ = windows.CloseHandle(group.handle)
}
