package program

import (
	"os/exec"
	"sync"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

// StartGroup starts command as the leader of its own process group (a job
// object on Windows) and returns the function that kills the whole group.
func StartGroup(command *exec.Cmd) (func(), error) {
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
	// The group is killed when the program exits and again by Kill: the
	// handle is closed once, since its value may be reused afterwards.
	var once sync.Once
	closeJob := func() {
		once.Do(func() {
			_ = windows.TerminateJobObject(job, 1)
			_ = windows.CloseHandle(job)
		})
	}
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.CREATE_NO_WINDOW}
	command.Cancel = func() error { return windows.TerminateJobObject(job, 1) }
	if err := command.Start(); err != nil {
		closeJob()
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
		closeJob()
		return nil, err
	}
	return closeJob, nil
}
