package link

func libp2pTestIdentity(first byte) Identity {
	var identity Identity
	for index := range identity.seed {
		identity.seed[index] = first + byte(index)
	}
	return identity
}
