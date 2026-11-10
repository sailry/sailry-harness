package protocol

type TerminalVT struct {
	Viewport TerminalViewport `json:"viewport"`
	Bytes    Bytes            `json:"bytes"`
}
