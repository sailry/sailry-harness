package git

import (
	"bufio"
	"bytes"
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"syscall"
	"time"

	gitconfig "github.com/go-git/go-git/v5/plumbing/format/config"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/unix"
)

type gitMount struct {
	path  string
	scope string
	write bool
	file  *os.File
}

// Linux Git reads executable settings from the worktree's own metadata. Put
// Git itself inside the boundary so every helper inherits the same restrictions,
// including helpers introduced after the command was admitted.
func prepareGitCommand(ctx context.Context, git *exec.Cmd, args []string, environment map[string]string, profile string) (gitCommand, error) {
	if git.Err != nil {
		return gitCommand{}, fault(protocol.ErrorCodeUnavailable, "Git executable unavailable on execution Node")
	}
	bwrap, err := gitSandboxLauncher()
	if err != nil {
		return gitCommand{}, err
	}
	root := filepath.Clean(git.Dir)
	read, _ := ctx.Value(gitReadKey{}).(bool)
	operation := gitOperation(args)
	transfer := operation == "clone" || operation == "fetch" || operation == "pull" || operation == "push" || operation == "ls-remote"
	arguments := []string{"--die-with-parent", "--unshare-pid", "--as-pid-1", "--unshare-ipc", "--unshare-uts", "--cap-drop", "ALL", "--new-session"}
	if !transfer {
		arguments = append(arguments, "--unshare-net", "--hostname", "localhost")
	}
	arguments = append(arguments, "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--tmpfs", "/.sailry-git", "--dir", "/.sailry-git/home", "--clearenv",
		"--setenv", "HOME", "/.sailry-git/home", "--setenv", "XDG_CONFIG_HOME", "/.sailry-git/home/.config", "--setenv", "TMPDIR", "/tmp",
		"--setenv", "PATH", "/usr/bin:/bin:/usr/local/bin", "--setenv", "LC_ALL", "C", "--setenv", "GIT_TERMINAL_PROMPT", "0",
		"--setenv", "GIT_EDITOR", "true", "--setenv", "GIT_SEQUENCE_EDITOR", "true", "--setenv", "GIT_PROTOCOL_FROM_USER", "0",
		"--setenv", "GIT_ALLOW_PROTOCOL", "file:git:http:https:ssh")
	system := []string{"/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc/passwd", "/etc/group", "/etc/ld.so.cache", "/etc/alternatives", "/etc/localtime", "/etc/gitconfig", "/etc/gitattributes"}
	if transfer {
		system = append(system, "/etc/hosts", "/etc/nsswitch.conf", "/etc/resolv.conf", "/etc/ssl/certs", "/etc/pki/tls/certs", "/etc/ssh/ssh_config", "/etc/ssh/ssh_config.d", "/etc/ssh/ssh_known_hosts")
	}
	for _, path := range system {
		if real, err := filepath.EvalSymlinks(path); err == nil {
			arguments = append(arguments, "--ro-bind", real, path)
		}
	}
	var mounts []gitMount
	writeRoot := !read && operation != "ls-remote"
	if operation == "clone" {
		// Clone's caller already created and admitted this exact destination.
		// Do not expose its siblings by mounting the destination's parent.
		if len(args) < 3 {
			return gitCommand{}, invalid("Git clone requires a source and destination")
		}
		root = args[len(args)-1]
		sourceIndex := len(args) - 2
		if source, uri := localGitCloneSource(args[sourceIndex]); source != "" {
			real, err := filepath.EvalSymlinks(source)
			if err != nil {
				return gitCommand{}, mapError(err)
			}
			mounts = append(mounts, gitMount{path: real, scope: real})
			// Git must address the same path that is visible in the sandbox.
			args = slices.Clone(args)
			args[sourceIndex] = real
			if uri != nil {
				uri.Path, uri.RawPath = real, ""
				args[sourceIndex] = uri.String()
			}
		}
	}
	if gitWritesObject(args) {
		writeRoot = true
	}
	if !gitScopeAllowed(root, environment["HOME"], profile) {
		return gitCommand{}, gitSandboxDenied()
	}
	rootFD, err := openGitScope(root)
	if err != nil {
		return gitCommand{}, gitSandboxDenied()
	}
	defer rootFD.Close()
	mounts = append(mounts, gitMount{path: root, scope: root, write: writeRoot, file: rootFD})
	metadata, err := linkedGitMetadata(rootFD, root, environment["HOME"], profile)
	if err != nil {
		return gitCommand{}, err
	}
	if metadata.file != nil {
		defer metadata.file.Close()
		metadata.write = writeRoot
		mounts = append(mounts, metadata)
	}
	if target, ok := ctx.Value(gitWorktreeKey{}).(string); ok && !pathWithin(target, root) {
		mounts = append(mounts, gitMount{path: target, scope: target, write: true})
	}
	for _, mount := range mounts {
		if !gitScopeAllowed(mount.scope, environment["HOME"], profile) {
			return gitCommand{}, gitSandboxDenied()
		}
	}
	// Parent mounts first, so an explicit source is read-only even when it
	// lives inside an admitted writable mount.
	slices.SortStableFunc(mounts, func(a, b gitMount) int {
		return strings.Count(a.path, string(filepath.Separator)) - strings.Count(b.path, string(filepath.Separator))
	})
	files := []*os.File{}
	closeFiles := func() {
		for _, file := range files {
			file.Close()
		}
	}
	for _, mount := range mounts {
		file := mount.file
		if file == nil {
			file, err = openGitScope(mount.path)
		} else {
			// Transfer a duplicate to exec.Cmd, keeping the same validated inode.
			var fd int
			fd, err = unix.FcntlInt(file.Fd(), unix.F_DUPFD_CLOEXEC, 0)
			if err == nil {
				file = os.NewFile(uintptr(fd), mount.path)
			}
		}
		if err != nil {
			closeFiles()
			return gitCommand{}, gitSandboxDenied()
		}
		flag := "--ro-bind-fd"
		if mount.write {
			flag = "--bind-fd"
		}
		arguments = append(arguments, flag, fmt.Sprint(3+len(files)), mount.path)
		files = append(files, file)
	}
	// Use a private synthetic home. Passing credentials or loading the real
	// account config would expose them to every repository-configured helper.
	globalConfig, err := gitGlobalSettings(environment)
	if err != nil {
		closeFiles()
		return gitCommand{}, mapError(err)
	}
	arguments = append(arguments, "--ro-bind-data", fmt.Sprint(3+len(files)), "/.sailry-git/home/.gitconfig")
	files = append(files, globalConfig)
	if !transfer {
		// Git resolves the hostname when deriving a fallback committer
		// identity, even when it only needs to write a branch's reflog.
		hosts, err := gitSandboxData([]byte("127.0.0.1 localhost\n::1 localhost\n"))
		if err != nil {
			closeFiles()
			return gitCommand{}, mapError(err)
		}
		arguments = append(arguments, "--ro-bind-data", fmt.Sprint(3+len(files)), "/etc/hosts")
		files = append(files, hosts)
	}
	info, sendInfo, err := os.Pipe()
	if err != nil {
		closeFiles()
		return gitCommand{}, mapError(err)
	}
	arguments = append(arguments, "--info-fd", fmt.Sprint(3+len(files)))
	files = append(files, sendInfo)
	arguments = append(arguments, "--chdir", root, "--", git.Path, "--no-pager", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false")
	arguments = append(arguments, args...)
	command := exec.Command(bwrap, arguments...)
	// In particular, do not pass loader or shell initialization variables to
	// the trusted launcher itself.
	command.Env = []string{"LC_ALL=C"}
	command.ExtraFiles = files
	launcher := gitCommand{Cmd: command, close: func() { closeFiles(); info.Close() }, started: closeFiles}
	launcher.stop = func() bool { return stopSandboxGit(info, command.Process.Pid) }
	return launcher, nil
}

func stopSandboxGit(info *os.File, launcherPID int) bool {
	// Bubblewrap reports the outside PID of Git, which runs as the namespace's
	// init. Signal Git first so it can remove its own reference or index locks.
	_ = info.SetReadDeadline(time.Now().Add(100 * time.Millisecond))
	var child struct {
		PID int `json:"child-pid"`
	}
	if json.NewDecoder(bufio.NewReader(info)).Decode(&child) != nil || child.PID <= 1 {
		return false
	}
	pid, err := unix.PidfdOpen(child.PID, 0)
	if err != nil {
		return false
	}
	defer unix.Close(pid)
	// A cancellation can arrive after the reported process exited. Confirm
	// that the PID still belongs to this launcher before sending a signal.
	status, err := os.ReadFile(fmt.Sprintf("/proc/%d/status", child.PID))
	if err != nil || !bytes.Contains(status, fmt.Appendf(nil, "\nPPid:\t%d\n", launcherPID)) {
		return false
	}
	return unix.PidfdSendSignal(pid, unix.SIGTERM, nil, 0) == nil
}

func openGitScope(path string) (*os.File, error) {
	root, err := os.Open("/")
	if err != nil {
		return nil, err
	}
	defer root.Close()
	fd, err := unix.Openat2(int(root.Fd()), strings.TrimPrefix(path, "/"), &unix.OpenHow{
		Flags:   unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC,
		Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS,
	})
	if err != nil {
		return nil, err
	}
	return os.NewFile(uintptr(fd), path), nil
}

func gitGlobalSettings(environment map[string]string) (*os.File, error) {
	home, xdg := environment["HOME"], environment["XDG_CONFIG_HOME"]
	if xdg == "" {
		xdg = filepath.Join(home, ".config")
	}
	settings := gitconfig.New()
	booleanValues := []string{"true", "false", "yes", "no", "on", "off", "1", "0"}
	for _, path := range []string{filepath.Join(xdg, "git", "config"), filepath.Join(home, ".gitconfig")} {
		if !filepath.IsAbs(path) {
			continue
		}
		file, err := os.OpenFile(path, os.O_RDONLY|syscall.O_NONBLOCK, 0)
		if err != nil {
			continue
		}
		values := gitconfig.New()
		err = gitconfig.NewDecoder(io.LimitReader(file, 1024*1024)).Decode(values)
		file.Close()
		if err != nil {
			continue
		}
		for _, section := range []string{"user", "author", "committer"} {
			for _, key := range []string{"name", "email"} {
				if value := values.Section(section).Option(key); value != "" && len(value) <= 256 && !strings.ContainsAny(value, "\r\n\x00") {
					settings.SetOption(section, gitconfig.NoSubsection, key, value)
				}
			}
		}
		// Keep signing intent and the standard LFS conversion. Other global
		// commands can carry credentials or rely on the host account's files.
		for _, section := range []string{"commit", "tag"} {
			if value := strings.ToLower(values.Section(section).Option("gpgsign")); slices.Contains(booleanValues, value) {
				settings.SetOption(section, gitconfig.NoSubsection, "gpgsign", value)
			}
		}
		lfs := values.Section("filter").Subsection("lfs")
		for _, conversion := range []struct{ key, command string }{
			{"clean", "git-lfs clean -- %f"},
			{"smudge", "git-lfs smudge -- %f"},
			{"process", "git-lfs filter-process"},
		} {
			if lfs.Option(conversion.key) == conversion.command {
				settings.SetOption("filter", "lfs", conversion.key, conversion.command)
			}
		}
		if value := strings.ToLower(lfs.Option("required")); slices.Contains(booleanValues, value) {
			settings.SetOption("filter", "lfs", "required", value)
		}
	}
	var contents bytes.Buffer
	if err := gitconfig.NewEncoder(&contents).Encode(settings); err != nil {
		return nil, err
	}
	return gitSandboxData(contents.Bytes())
}

func gitSandboxData(contents []byte) (*os.File, error) {
	read, write, err := os.Pipe()
	if err != nil {
		return nil, err
	}
	_, err = write.Write(contents)
	write.Close()
	if err != nil {
		read.Close()
		return nil, err
	}
	return read, nil
}

type gitWorktreeKey struct{}

func prepareGitWorktree(ctx context.Context, path string) (context.Context, func(), error) {
	if err := os.Mkdir(path, 0o777); err != nil {
		if errors.Is(err, os.ErrExist) {
			return nil, nil, fault(protocol.ErrorCodeConflict, DestinationExists)
		}
		return nil, nil, mapError(err)
	}
	return context.WithValue(ctx, gitWorktreeKey{}, path), func() { _ = os.Remove(path) }, nil
}

func (s *Service) removeGitWorktree(ctx context.Context, main, root string) error {
	removeRegistration := func() error {
		_, err := s.command(ctx, main, []string{"worktree", "remove", "--", root}, nil, false)
		return err
	}
	if pathWithin(root, main) {
		return removeRegistration()
	}
	// The approved external checkout is a mountpoint in the Git sandbox,
	// which Git cannot remove. Move that exact checkout out of the way while
	// Git removes its registration. Keep the checkout if the outcome is unclear.
	parent, name := filepath.Dir(root), filepath.Base(root)
	parentFD, err := openGitScope(parent)
	if err != nil {
		return mapError(err)
	}
	defer parentFD.Close()
	parentDescriptor := fmt.Sprintf("/proc/self/fd/%d", parentFD.Fd())
	anchor, err := os.OpenRoot(parentDescriptor)
	if err != nil {
		return mapError(err)
	}
	defer anchor.Close()
	directory := ".sailry-git-remove-" + rand.Text()
	if err := anchor.Mkdir(directory, 0o700); err != nil {
		return mapError(err)
	}
	defer anchor.Remove(directory)
	recovery := filepath.Join(directory, name)
	preserved := func(message string) error {
		location, err := os.Readlink(parentDescriptor)
		if err != nil {
			location = parent
		}
		return fault(protocol.ErrorCodeOutcomeUnknown, message+filepath.Join(location, recovery))
	}
	admitted, err := anchor.Lstat(name)
	if err != nil || !admitted.IsDir() {
		return conflict()
	}
	if err := anchor.Rename(name, recovery); err != nil {
		return mapError(err)
	}
	rollback := func() bool {
		fd := int(parentFD.Fd())
		return unix.Renameat2(fd, recovery, fd, name, unix.RENAME_NOREPLACE) == nil
	}
	if moved, err := anchor.Lstat(recovery); err != nil || !os.SameFile(admitted, moved) {
		if rollback() {
			return conflict()
		}
		return preserved("Git worktree changed during removal; the checkout is preserved at ")
	}
	if err := removeRegistration(); err != nil {
		var value *protocol.Fault
		if !errors.As(err, &value) || value.Code == protocol.ErrorCodeOutcomeUnknown || value.Code == protocol.ErrorCodeCancelled {
			return preserved("Git worktree removal outcome is unknown; the checkout is preserved at ")
		}
		if rollback() {
			return err
		}
		return preserved("Git worktree removal did not complete; the checkout is preserved at ")
	}
	if err := anchor.RemoveAll(recovery); err != nil {
		return preserved("Git worktree registration was removed; remaining files are preserved at ")
	}
	return nil
}

func gitOperation(args []string) string {
	for index := 0; index < len(args); index++ {
		if args[index] == "-c" || args[index] == "-C" {
			index++
		} else if !strings.HasPrefix(args[index], "-") {
			return args[index]
		}
	}
	return ""
}

func gitWritesObject(args []string) bool {
	return gitOperation(args) == "hash-object" && slices.Contains(args, "-w")
}

func localGitCloneSource(value string) (string, *url.URL) {
	if filepath.IsAbs(value) {
		return value, nil
	}
	parsed, err := url.Parse(value)
	if err == nil && parsed.Scheme == "file" && (parsed.Host == "" || parsed.Host == "localhost") && filepath.IsAbs(parsed.Path) {
		return parsed.Path, parsed
	}
	return "", nil
}

func gitSandboxDenied() error {
	return fault(protocol.ErrorCodePermissionDenied, "Git worktree or metadata overlaps data outside the Linux Git sandbox scope")
}

func gitScopeAllowed(path, home, profile string) bool {
	if !filepath.IsAbs(path) || filepath.Dir(path) == path {
		return false
	}
	for _, system := range []string{"/bin", "/dev", "/etc", "/lib", "/lib64", "/proc", "/run", "/sbin", "/sys", "/usr", "/var/run", "/.sailry-git"} {
		if pathWithin(path, system) || pathWithin(system, path) {
			return false
		}
	}
	if path == "/tmp" {
		return false
	}
	if home, err := filepath.EvalSymlinks(home); home != "" && err == nil {
		if pathWithin(home, path) || pathWithin(path, filepath.Join(home, ".ssh")) || pathWithin(path, filepath.Join(home, ".gnupg")) || pathWithin(path, filepath.Join(home, ".config")) {
			return false
		}
	}
	if profile, err := filepath.EvalSymlinks(profile); profile != "" && err == nil {
		if pathWithin(profile, path) || pathWithin(path, profile) && !core.ManagedWorkspace(path, profile) {
			return false
		}
	}
	return true
}

var gitSandboxLauncher = sync.OnceValues(func() (string, error) {
	for _, candidate := range []string{"/usr/bin/bwrap", "/bin/bwrap"} {
		path, err := filepath.EvalSymlinks(candidate)
		if err != nil {
			continue
		}
		info, err := os.Stat(path)
		if err != nil {
			continue
		}
		if stat, ok := info.Sys().(*syscall.Stat_t); ok && info.Mode().IsRegular() && info.Mode().Perm()&0o022 == 0 && stat.Uid == 0 {
			command := exec.Command(path, "--help")
			command.Env = []string{"LC_ALL=C"}
			out, err := command.Output()
			if err == nil && bytes.Contains(out, []byte("--bind-fd")) && bytes.Contains(out, []byte("--ro-bind-fd")) {
				return path, nil
			}
			continue
		}
	}
	return "", fault(protocol.ErrorCodeUnavailable, "Git requires a trusted Linux Bubblewrap launcher with file-descriptor mounts")
})

func linkedGitMetadata(rootFD *os.File, root, home, profile string) (gitMount, error) {
	dot := filepath.Join(root, ".git")
	var info unix.Stat_t
	err := unix.Fstatat(int(rootFD.Fd()), ".git", &info, unix.AT_SYMLINK_NOFOLLOW)
	if errors.Is(err, os.ErrNotExist) || err == nil && info.Mode&unix.S_IFMT == unix.S_IFDIR {
		return gitMount{}, nil
	}
	if err != nil || info.Mode&unix.S_IFMT != unix.S_IFREG {
		return gitMount{}, gitSandboxDenied()
	}
	pointer, ok := gitMetadataText(rootFD, ".git", 4096)
	if !ok || !strings.HasPrefix(pointer, "gitdir: ") {
		return gitMount{}, gitSandboxDenied()
	}
	admin := gitMetadataPath(root, strings.TrimPrefix(pointer, "gitdir: "))
	if admin == "" {
		return gitMount{}, gitSandboxDenied()
	}
	if pathWithin(admin, root) {
		return gitMount{}, nil
	}
	// Git records reciprocal pointers in the common metadata directory. A
	// linked checkout of a bare repository uses the repository itself as the
	// common directory. Keep that mount confined to the exact bare directory.
	parent := filepath.Dir(admin)
	common := filepath.Dir(parent)
	ordinary := filepath.Base(common) == ".git"
	scope := common
	if ordinary {
		scope = filepath.Dir(common)
	}
	if filepath.Base(parent) != "worktrees" || !gitScopeAllowed(scope, home, profile) {
		return gitMount{}, gitSandboxDenied()
	}
	commonFD, err := openGitScope(common)
	if err != nil {
		return gitMount{}, gitSandboxDenied()
	}
	entry := filepath.Join("worktrees", filepath.Base(admin))
	backlink, hasBacklink := gitMetadataText(commonFD, filepath.Join(entry, "gitdir"), 4096)
	commonPointer, hasCommon := gitMetadataText(commonFD, filepath.Join(entry, "commondir"), 4096)
	if !hasBacklink || gitMetadataPath(admin, backlink) != dot || !hasCommon || gitMetadataPath(admin, commonPointer) != common ||
		!ordinary && !bareGitMetadata(commonFD) {
		commonFD.Close()
		return gitMount{}, gitSandboxDenied()
	}
	return gitMount{path: common, scope: scope, file: commonFD}, nil
}

func bareGitMetadata(root *os.File) bool {
	config, ok := gitMetadataText(root, "config", 1024*1024)
	if !ok {
		return false
	}
	settings := gitconfig.New()
	if err := gitconfig.NewDecoder(strings.NewReader(config)).Decode(settings); err != nil {
		return false
	}
	switch strings.ToLower(settings.Section("core").Option("bare")) {
	case "true", "yes", "on", "1":
	default:
		return false
	}
	if head, ok := gitMetadataText(root, "HEAD", 4096); !ok || head == "" {
		return false
	}
	for _, entry := range []string{"objects", "refs"} {
		var info unix.Stat_t
		if unix.Fstatat(int(root.Fd()), entry, &info, unix.AT_SYMLINK_NOFOLLOW) != nil || info.Mode&unix.S_IFMT != unix.S_IFDIR {
			return false
		}
	}
	return true
}

func gitMetadataPath(parent, value string) string {
	if value == "" {
		return ""
	}
	if filepath.IsAbs(value) {
		return filepath.Clean(value)
	}
	return filepath.Join(parent, value)
}

func gitMetadataText(root *os.File, path string, limit int64) (string, bool) {
	fd, err := unix.Openat2(int(root.Fd()), path, &unix.OpenHow{
		Flags:   unix.O_RDONLY | unix.O_CLOEXEC | unix.O_NONBLOCK,
		Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS,
	})
	if err != nil {
		return "", false
	}
	file := os.NewFile(uintptr(fd), path)
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() > limit {
		return "", false
	}
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil || int64(len(data)) > limit || bytes.IndexByte(data, 0) >= 0 {
		return "", false
	}
	return strings.TrimSuffix(strings.TrimSuffix(string(data), "\n"), "\r"), true
}
