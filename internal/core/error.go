package core

import (
	"errors"
	"fmt"
)

var (
	ErrStopped         = errors.New("Node service is stopping or stopped")
	ErrBusy            = errors.New("Node task capacity is exhausted")
	ErrShutdownTimeout = errors.New("Node shutdown deadline elapsed; remaining tasks were cancelled")
)

type ProfileInUseError struct {
	Path string
}

func (err *ProfileInUseError) Error() string {
	return fmt.Sprintf("a Node already owns profile %s", err.Path)
}

type InvalidProfileError struct {
	Path   string
	Reason string
}

func (err *InvalidProfileError) Error() string {
	return fmt.Sprintf("invalid profile %s: %s", err.Path, err.Reason)
}

type WorkerError struct {
	Err error
}

func (err *WorkerError) Error() string { return fmt.Sprintf("Node worker failed: %v", err.Err) }
func (err *WorkerError) Unwrap() error { return err.Err }

func invalidProfile(path, reason string) error {
	return &InvalidProfileError{Path: path, Reason: reason}
}
