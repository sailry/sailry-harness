//go:build !darwin

package capture

import "context"

// Status is unknown where the application cannot read an authorization.
func Status() Permission { return PermissionUnknown }

// Request has no prompt to raise; capture is attempted.
func Request(context.Context) (bool, error) { return true, nil }
