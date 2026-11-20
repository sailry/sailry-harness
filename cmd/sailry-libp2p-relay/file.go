package main

import (
	"errors"
	"fmt"
	"io"
	"os"
)

func openRelayRegularFile(path string, limit int64) (*os.File, os.FileInfo, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, nil, errors.New("expected a regular file; symbolic links are not allowed")
	}
	if info.Size() > limit {
		return nil, nil, fmt.Errorf("file exceeds the %d-byte limit", limit)
	}
	file, err := os.OpenFile(path, relayFileReadFlags, 0)
	if err != nil {
		return nil, nil, err
	}
	opened, err := file.Stat()
	if err == nil && (!opened.Mode().IsRegular() || !os.SameFile(info, opened) || opened.Size() != info.Size()) {
		err = errors.New("file changed before it could be read")
	}
	if err != nil {
		file.Close()
		return nil, nil, err
	}
	return file, opened, nil
}

func copyRelayRegularFile(output io.Writer, file *os.File, size int64) error {
	n, err := io.Copy(output, io.LimitReader(file, size+1))
	if err != nil {
		return err
	}
	if n != size {
		return errors.New("file size changed while it was read")
	}
	return nil
}
