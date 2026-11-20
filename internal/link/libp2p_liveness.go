package link

// The inspection has already closed its stream. A missing reply identifies
// neither a path that failed before opening nor the status of another Link
// stream (including one still running on this same circuit).
func discardIdleProbePath(probe contextStream) {
	stream, ok := probe.(*libp2pStream)
	if !ok {
		return
	}
	path := stream.raw.Conn()
	if path.IsClosed() {
		return
	}
	for _, other := range path.GetStreams() {
		switch other.Protocol() {
		case libp2pProtocol(libp2pNodeALPN), libp2pProtocol(libp2pPairALPN):
			return
		}
	}
	_ = path.Close()
}
