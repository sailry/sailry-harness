package preferences

import (
	"errors"
	"os"
)

type writeFailure struct {
	stage string
	err   error
}

func (e *writeFailure) Error() string {
	_, reason := WriteFailure(e)
	return "write desktop preferences: " + e.stage + "/" + reason
}

func (e *writeFailure) Unwrap() error { return e.err }

// WriteFailure exposes only the fixed write step and system error category.
// The underlying error remains available through errors.Is and errors.As.
func WriteFailure(err error) (stage, reason string) {
	var failure *writeFailure
	if !errors.As(err, &failure) || failure == nil {
		return "", ""
	}
	switch {
	case errors.Is(failure.err, os.ErrPermission):
		reason = "permission_denied"
	case errors.Is(failure.err, os.ErrNotExist):
		reason = "not_found"
	case errors.Is(failure.err, os.ErrExist):
		reason = "already_exists"
	default:
		reason = writeFailureReason(failure.err)
	}
	return failure.stage, reason
}
