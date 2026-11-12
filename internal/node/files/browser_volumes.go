package files

import (
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/disk"
)

// pseudoFileSystems hold no files of the user's: the kernel's views, memory
// and the system's own images. gopsutil leaves most of them out on Linux and
// none on macOS.
var pseudoFileSystems = map[string]bool{
	"autofs": true, "bpf": true, "binfmt_misc": true, "cgroup": true, "cgroup2": true, "configfs": true, "debugfs": true,
	"devfs": true, "devpts": true, "devtmpfs": true, "efivarfs": true, "fdescfs": true, "fusectl": true, "hugetlbfs": true,
	"mqueue": true, "nsfs": true, "nullfs": true, "proc": true, "procfs": true, "pstore": true, "ramfs": true, "rootfs": true,
	"rpc_pipefs": true, "securityfs": true, "squashfs": true, "sysfs": true, "tmpfs": true, "tracefs": true,
}

// systemMounts are where a system mounts its own volumes: macOS's sealed and
// hidden ones (Data, VM, Preboot, Update), which share the startup volume's
// space, and the kernel's directories on Linux.
var systemMounts = []string{"/System/Volumes", "/private/var", "/dev", "/proc", "/sys", "/run"}

// IsVolume reports whether a mount of goos is a volume the user stores files
// on, the one rule of the file browser's devices and of the host metrics'
// disks: not a pseudo file system, not one of the system's own mounts
// (removable media under /run/media stay), and on macOS not one the system
// hides from its file browser (nobrowse: /dev, /System/Volumes/*, Recovery).
func IsVolume(goos string, mount disk.PartitionStat) bool {
	path := mount.Mountpoint
	if path == "" || pseudoFileSystems[strings.ToLower(mount.Fstype)] {
		return false
	}
	if goos == "windows" {
		return true
	}
	if goos == "darwin" && slices.Contains(mount.Opts, "nobrowse") {
		return false
	}
	if strings.HasPrefix(path, "/run/media/") {
		return true
	}
	for _, prefix := range systemMounts {
		if path == prefix || strings.HasPrefix(path, prefix+"/") {
			return false
		}
	}
	return true
}

// volumes are the devices a file manager of goos shows, out of the mounts
// that are volumes (IsVolume), which the host metrics list too: on macOS the
// startup volume under its name and the volumes mounted in /Volumes; on
// Linux the root and the media mounted for account under /media and
// /run/media, and in /mnt; on Windows the drive letters. detail names a
// Windows drive and tells its medium.
func volumes(goos string, mounts []disk.PartitionStat, account, startup string, detail func(root string) (string, *protocol.FileBrowserMedium)) []protocol.FileBrowserLocation {
	medium := func(value protocol.FileBrowserMedium) *protocol.FileBrowserMedium { return &value }
	var out []protocol.FileBrowserLocation
	seen := map[string]bool{}
	add := func(location protocol.FileBrowserLocation) {
		if !seen[location.Path] {
			seen[location.Path] = true
			out = append(out, location)
		}
	}
	mounts = slices.DeleteFunc(slices.Clone(mounts), func(mount disk.PartitionStat) bool { return !IsVolume(goos, mount) })
	if goos != "windows" {
		for _, mount := range mounts {
			if mount.Mountpoint == "/" {
				add(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindRoot, Name: startup, Path: "/", Medium: medium(protocol.FileBrowserMediumInternal)})
			}
		}
	}
	for _, mount := range mounts {
		path := mount.Mountpoint
		switch goos {
		case "windows":
			if len(path) != 2 || path[1] != ':' {
				continue
			}
			root := path + `\`
			label, kind := detail(root)
			name := path
			if label != "" {
				name = label + " (" + path + ")"
			}
			add(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: name, Path: root, Medium: kind})
		case "darwin":
			name := filepath.Base(path)
			if filepath.Dir(path) != "/Volumes" || strings.HasPrefix(name, ".") {
				continue
			}
			location := protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: name, Path: path}
			if !slices.Contains(mount.Opts, "local") {
				location.Medium = medium(protocol.FileBrowserMediumNetwork)
			}
			add(location)
		case "linux":
			parent, name := filepath.Dir(path), filepath.Base(path)
			location := protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: name, Path: path}
			switch {
			case strings.HasPrefix(name, "."):
				continue
			case account != "" && (parent == "/media/"+account || parent == "/run/media/"+account):
				location.Medium = medium(protocol.FileBrowserMediumExternal)
			case parent != "/mnt":
				continue
			}
			add(location)
		}
	}
	return out
}

// startupName is the name of the startup volume of macOS: the link to / the
// system keeps beside the other volumes, in directory (/Volumes). It is ""
// where there is none.
func startupName(directory string) string {
	entries, err := os.ReadDir(directory)
	if err != nil {
		return ""
	}
	for _, entry := range entries {
		if entry.Type()&os.ModeSymlink == 0 {
			continue
		}
		if target, err := os.Readlink(filepath.Join(directory, entry.Name())); err == nil && target == "/" {
			return entry.Name()
		}
	}
	return ""
}
