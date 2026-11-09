//go:build unix

package core

import (
	"io/fs"
	"os"
	"syscall"

	"golang.org/x/sys/unix"
)

type nativeFileIdentity struct {
	Device uint64 `json:"device"`
	Inode  uint64 `json:"inode"`
}

func fileNativeIdentity(file *os.File) (nativeFileIdentity, error) {
	metadata, err := file.Stat()
	if err != nil {
		return nativeFileIdentity{}, fileIOFault(err)
	}
	stat := metadata.Sys().(*syscall.Stat_t)
	return nativeFileIdentity{Device: uint64(stat.Dev), Inode: uint64(stat.Ino)}, nil
}

func regularFileFlags() int   { return os.O_RDONLY | unix.O_NOFOLLOW | unix.O_NONBLOCK }
func exclusiveFileFlags() int { return os.O_WRONLY | os.O_CREATE | os.O_EXCL | unix.O_NOFOLLOW }

func fileReadonly(metadata fs.FileInfo) bool { return metadata.Mode().Perm()&0o222 == 0 }

func syncFileDirectory(root *os.Root) error {
	file, err := root.Open(".")
	if err != nil {
		return err
	}
	defer file.Close()
	return file.Sync()
}

func renameFileBetween(source *os.Root, from string, target *os.Root, to string) error {
	sourceDirectory, err := source.Open(".")
	if err != nil {
		return err
	}
	defer sourceDirectory.Close()
	targetDirectory, err := target.Open(".")
	if err != nil {
		return err
	}
	defer targetDirectory.Close()
	return unix.Renameat(int(sourceDirectory.Fd()), from, int(targetDirectory.Fd()), to)
}
