package link

// retireControllerImportedRoutes keeps old directory records for provenance
// while removing the execution direction they used to introduce implicitly.
func retireControllerImportedRoutes(peers []controllerPeer) bool {
	changed := false
	for index := range peers {
		peer := &peers[index]
		if peer.Removed {
			continue
		}
		if !peer.Direct && !peer.Direction.AllowsInbound() {
			if !peer.DirectoryOnly {
				peer.DirectoryOnly = true
				changed = true
			}
			continue
		}
		if peer.Direction == DirectionBoth && (!peer.Direct || !peer.DirectDirection.AllowsOutbound()) {
			peer.Direction = DirectionController
			changed = true
		}
	}
	return changed
}
