package panel

// TakeOpened returns the kinds of the tabs opened since the last call, for
// the anonymous feature counters: kinds only, never a tab's file, address
// or session.
func (s *State) TakeOpened() []Kind {
	opened := s.opened
	s.opened = nil
	return opened
}
