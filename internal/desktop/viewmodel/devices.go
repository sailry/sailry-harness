package viewmodel

type Device struct {
	ID, Name, Platform string
	Execution, Online  bool
	Local              bool
	Route              string
	RTT                int
	// OS is the operating system's version and Version the Sailry release
	// the device runs; each is empty until the device reports it.
	OS, Version string
}
