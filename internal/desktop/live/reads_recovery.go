package live

func (r *Runtime) resumeReadsOnReconnect() {
	connected := r.controller.Connected()
	if connected && !r.readsConnected {
		r.retryFailedReads()
	}
	r.readsConnected = connected
}

// retryFailedReads is driven by a reconnected Node or the user's Refresh.
// Client owns recovery inside a subscription; reads that have ended wait for
// one of these events, so a persistent fault cannot turn drawing into polling.
func (r *Runtime) retryFailedReads() {
	if !r.controller.Connected() || r.ctx.Err() != nil {
		return
	}
	if r.attachments != nil {
		for _, cache := range r.attachments.imageSessions {
			clear(cache.failed)
		}
	}
	if state := r.packages; state != nil {
		if state.installFailed {
			state.installFailed = false
			r.observeInstallations()
		}
		if state.pythonFailed {
			state.pythonFailed = false
			r.observePython()
		}
		if state.pythonReadFailed {
			state.pythonReadFailed = false
			r.readPython()
		}
	}
	if state := r.authorizations; state != nil && state.connectionsFailed {
		state.connectionsFailed = false
		r.initConnectorUI()
	}
}
