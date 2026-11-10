package viewmodel

type Device struct {
	ID, Name, Platform string
	Execution, Online  bool
	Local              bool
	Route              string
	RTT                int
}
