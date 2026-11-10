package computer

import (
	"os"
	"sync/atomic"
	"syscall"

	"github.com/ebitengine/purego"
)

// macOS charges TCC grants to a process tree's responsible application.
// Disclaiming makes the signed helper application, rather than Desktop or
// Host, own its Accessibility and Screen Recording grants. The disclaimer is
// a spawn attribute of the process that replaces its own image, so the Node
// executable starts itself as a trampoline that becomes the helper.

const (
	trampoline   = "--sailry-computer-helper"
	spawnSetExec = 0x0040
)

var trampolines atomic.Bool

// Trampoline must run first in a Node executable's main. In a trampoline
// process it becomes the named helper and does not return; otherwise it
// enables disclaimed helper launches for this process.
func Trampoline() {
	if len(os.Args) > 2 && os.Args[1] == trampoline {
		become(os.Args[2], os.Args[2:], os.Environ())
	}
	trampolines.Store(true)
}

// Disclaim rewrites a helper launch to pass through the trampoline. Without
// a trampoline, as in a test executable, the launch is unchanged.
func Disclaim(command string, args []string) (string, []string) {
	if !trampolines.Load() {
		return command, args
	}
	self, err := os.Executable()
	if err != nil {
		return command, args
	}
	return self, append([]string{trampoline, command}, args...)
}

// become replaces this process with program under a disclaimed
// responsibility. A failed preparation keeps the ordinary exec, so
// diagnostics and MCP still observe the same helper.
func become(program string, argv, env []string) {
	disclaim(program, argv, env)
	_ = syscall.Exec(program, argv, env)
	os.Exit(127)
}

func disclaim(program string, argv, env []string) {
	symbol := func(name string) uintptr {
		address, err := purego.Dlsym(purego.RTLD_DEFAULT, name)
		if err != nil {
			return 0
		}
		return address
	}
	initialize, flags := symbol("posix_spawnattr_init"), symbol("posix_spawnattr_setflags")
	responsibility, replace := symbol("responsibility_spawnattrs_setdisclaim"), symbol("posix_spawn")
	if initialize == 0 || flags == 0 || responsibility == 0 || replace == 0 {
		return
	}
	var (
		attributesInit func(attributes *uintptr) int32
		setFlags       func(attributes *uintptr, flags int16) int32
		setDisclaim    func(attributes *uintptr, disclaim int32) int32
		spawn          func(pid *int32, path *byte, actions uintptr, attributes *uintptr, argv, envp **byte) int32
	)
	purego.RegisterFunc(&attributesInit, initialize)
	purego.RegisterFunc(&setFlags, flags)
	purego.RegisterFunc(&setDisclaim, responsibility)
	purego.RegisterFunc(&spawn, replace)
	path, err := syscall.BytePtrFromString(program)
	if err != nil {
		return
	}
	arguments, err := syscall.SlicePtrFromStrings(argv)
	if err != nil {
		return
	}
	environment, err := syscall.SlicePtrFromStrings(env)
	if err != nil {
		return
	}
	var attributes uintptr
	if attributesInit(&attributes) != 0 || setFlags(&attributes, spawnSetExec) != 0 || setDisclaim(&attributes, 1) != 0 {
		return
	}
	// With POSIX_SPAWN_SETEXEC the call replaces this process image and
	// returns only on failure.
	spawn(nil, path, 0, &attributes, &arguments[0], &environment[0])
}
