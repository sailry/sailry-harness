package capture

// Permission is the system's microphone authorization for this application.
type Permission int

const (
	// PermissionUnknown is every system without an authorization the
	// application can read; capture is attempted there.
	PermissionUnknown Permission = iota
	PermissionNotDetermined
	PermissionGranted
	PermissionDenied
	PermissionRestricted
)

// Allowed reports whether capture may be attempted: the user allowed it, or
// the system has no authorization to read.
func (p Permission) Allowed() bool { return p == PermissionGranted || p == PermissionUnknown }
