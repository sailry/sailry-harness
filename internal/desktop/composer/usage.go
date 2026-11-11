package composer

// ModelPickerOpen reports whether the model controls are open, for the
// anonymous feature counters.
func (s *State) ModelPickerOpen() bool { return s.picker }
