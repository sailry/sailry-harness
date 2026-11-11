package process

import (
	"errors"
	"io"
	"maps"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"unicode/utf16"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

var updateProcThreadAttribute = encodingKernel.NewProc("UpdateProcThreadAttribute")

func launchNative(launch Launch, environment Environment, _ func(string) (string, error)) (_ *nativeProcess, failure error) {
	directory, err := commandDirectory(launch)
	if err != nil {
		return nil, err
	}
	if strings.HasPrefix(directory, `\\?\`) && len(directory) > 6 && directory[5] == ':' {
		directory = directory[4:]
	}
	values := environment.commandValues(launch.Access)
	if values == nil {
		values = map[string]string{}
	}
	maps.Copy(values, launch.Environment)
	if launch.Inputs != "" {
		values["SAILRY_ATTACHMENTS"] = launch.Inputs
	}
	if launch.Scratch != "" {
		for _, name := range []string{"TMPDIR", "TMP", "TEMP"} {
			values[name] = launch.Scratch
		}
	}
	values["PWD"] = directory
	if launch.Pty {
		values["TERM"] = ptyTerm
	}
	shell := environment.Shell()
	if strings.EqualFold(shell, "cmd.exe") {
		system, err := windows.GetSystemDirectory()
		if err != nil {
			return nil, err
		}
		shell = filepath.Join(system, "cmd.exe")
	}
	application, err := windows.UTF16PtrFromString(shell)
	if err != nil {
		return nil, err
	}
	line, err := windows.UTF16PtrFromString(windows.EscapeArg(shell) + ` /D /S /C "` + launch.Command + `"`)
	if err != nil {
		return nil, err
	}
	cwd, err := windows.UTF16PtrFromString(directory)
	if err != nil {
		return nil, err
	}
	entries := environmentValues(values)
	slices.SortFunc(entries, func(a, b string) int { return strings.Compare(strings.ToUpper(a), strings.ToUpper(b)) })
	block := utf16.Encode([]rune(strings.Join(entries, "\x00") + "\x00\x00"))
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	var handles []windows.Handle
	handles = append(handles, job)
	var owned []io.Closer
	var console windows.Handle
	var closeConsoleOnce sync.Once
	closeConsole := func() {
		closeConsoleOnce.Do(func() {
			if console != 0 {
				windows.ClosePseudoConsole(console)
			}
		})
	}
	var closeOnce sync.Once
	closeAll := func() {
		closeOnce.Do(func() {
			closeConsole()
			for _, stream := range owned {
				_ = stream.Close()
			}
			for _, handle := range handles {
				_ = windows.CloseHandle(handle)
			}
		})
	}
	defer func() {
		if failure != nil {
			closeAll()
		}
	}()
	limits := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limits.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err = windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limits)), uint32(unsafe.Sizeof(limits))); err != nil {
		return nil, err
	}
	attributes, err := windows.NewProcThreadAttributeList(1)
	if err != nil {
		return nil, err
	}
	defer attributes.Delete()
	startup := windows.StartupInfoEx{StartupInfo: windows.StartupInfo{Cb: uint32(unsafe.Sizeof(windows.StartupInfoEx{}))}, ProcThreadAttributeList: attributes.List()}
	var stdout, stderr io.ReadCloser
	var stdin io.WriteCloser
	var childPipes []*os.File
	inherit := false
	if launch.Pty {
		inReader, inWriter, err := os.Pipe()
		if err != nil {
			return nil, err
		}
		owned = append(owned, inReader, inWriter)
		outReader, outWriter, err := os.Pipe()
		if err != nil {
			return nil, err
		}
		owned = append(owned, outReader, outWriter)
		if err = windows.CreatePseudoConsole(windows.Coord{X: ptyColumns, Y: ptyRows}, windows.Handle(inReader.Fd()), windows.Handle(outWriter.Fd()), 0, &console); err != nil {
			return nil, err
		}
		// This attribute's value is the console handle itself rather than
		// a pointer to it, which Update's unsafe.Pointer cannot carry.
		if result, _, err := updateProcThreadAttribute.Call(uintptr(unsafe.Pointer(attributes.List())), 0, windows.PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, uintptr(console), unsafe.Sizeof(console), 0, 0); result == 0 {
			return nil, err
		}
		childPipes = []*os.File{inReader, outWriter}
		stdout, stdin = outReader, inWriter
	} else {
		outReader, outWriter, err := os.Pipe()
		if err != nil {
			return nil, err
		}
		owned = append(owned, outReader, outWriter)
		errReader, errWriter, err := os.Pipe()
		if err != nil {
			return nil, err
		}
		owned = append(owned, errReader, errWriter)
		var inReader *os.File
		if launch.Background {
			var inWriter *os.File
			inReader, inWriter, err = os.Pipe()
			if err != nil {
				return nil, err
			}
			owned = append(owned, inReader, inWriter)
			stdin = inWriter
		} else {
			inReader, err = os.Open(os.DevNull)
			if err != nil {
				return nil, err
			}
			owned = append(owned, inReader)
		}
		childPipes = []*os.File{inReader, outWriter, errWriter}
		inherited := make([]windows.Handle, len(childPipes))
		for index, file := range childPipes {
			inherited[index] = windows.Handle(file.Fd())
			if err = windows.SetHandleInformation(inherited[index], windows.HANDLE_FLAG_INHERIT, windows.HANDLE_FLAG_INHERIT); err != nil {
				return nil, err
			}
		}
		if err = attributes.Update(windows.PROC_THREAD_ATTRIBUTE_HANDLE_LIST, unsafe.Pointer(&inherited[0]), uintptr(len(inherited))*unsafe.Sizeof(inherited[0])); err != nil {
			return nil, err
		}
		startup.Flags = windows.STARTF_USESTDHANDLES | windows.STARTF_USESHOWWINDOW
		startup.ShowWindow = windows.SW_HIDE
		startup.StdInput, startup.StdOutput, startup.StdErr = inherited[0], inherited[1], inherited[2]
		stdout, stderr = outReader, errReader
		inherit = true
	}
	var information windows.ProcessInformation
	flags := uint32(windows.CREATE_SUSPENDED | windows.CREATE_UNICODE_ENVIRONMENT | windows.EXTENDED_STARTUPINFO_PRESENT | windows.CREATE_NEW_PROCESS_GROUP)
	if !launch.Pty {
		flags |= windows.CREATE_NO_WINDOW
	}
	if err = windows.CreateProcess(application, line, nil, nil, inherit, flags, &block[0], cwd, &startup.StartupInfo, &information); err != nil {
		return nil, err
	}
	handles = append(handles, information.Process, information.Thread)
	// The root is suspended until its private kill-on-close Job owns it.
	if err = windows.AssignProcessToJobObject(job, information.Process); err != nil {
		_ = windows.TerminateProcess(information.Process, 1)
		return nil, err
	}
	if _, err = windows.ResumeThread(information.Thread); err != nil {
		_ = windows.TerminateJobObject(job, 1)
		return nil, err
	}
	runtime.KeepAlive(block)
	for _, file := range childPipes {
		_ = file.Close()
	}
	var killOnce sync.Once
	var killError error
	return &nativeProcess{stdout: stdout, stderr: stderr, stdin: stdin, close: closeAll, finishOutput: closeConsole,
		kill: func() error { killOnce.Do(func() { killError = windows.TerminateJobObject(job, 1) }); return killError },
		settled: func() bool {
			state, err := windows.WaitForSingleObject(information.Process, 0)
			return err == nil && state == windows.WAIT_OBJECT_0
		},
		wait: func() (protocol.ProcessOutcome, error) {
			if _, err := windows.WaitForSingleObject(information.Process, windows.INFINITE); err != nil {
				return protocol.ProcessOutcome{}, err
			}
			var code uint32
			if err := windows.GetExitCodeProcess(information.Process, &code); err != nil {
				return protocol.ProcessOutcome{}, err
			}
			return protocol.ProcessOutcome{Kind: "exited", Data: int32(code)}, nil
		}}, nil
}

func nativeReadEnd(err error, _ bool) bool {
	return errors.Is(err, io.EOF) || errors.Is(err, os.ErrClosed) || errors.Is(err, windows.ERROR_BROKEN_PIPE) || errors.Is(err, windows.ERROR_OPERATION_ABORTED)
}
