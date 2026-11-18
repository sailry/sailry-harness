package live

func (r *Runtime) resumeReadsOnReconnect() {
	connected := r.controller.Connected()
	if connected && !r.readsConnected {
		r.recoverFailedReads(false)
	} else if connected {
		// A replacement binding can already be connected at its first frame.
		r.retryControllerLogins(false)
	}
	r.readsConnected = connected
}

func (r *Runtime) retryFailedReads() {
	r.recoverFailedReads(true)
}

// recoverFailedReads is driven by a reconnected Node or the user's Refresh.
// Client owns recovery inside a subscription; reads that have ended wait for
// one of these events, so a persistent fault cannot turn drawing into polling.
func (r *Runtime) recoverFailedReads(refresh bool) {
	if !r.controller.Connected() || r.ctx.Err() != nil {
		return
	}
	r.retryControllerLogins(true)
	for _, pending := range r.conversationCreations {
		pending.resume()
	}
	if r.workspace != nil {
		node, projectID, overview := r.localRuntime().App.ProjectOwner()
		for _, project := range r.workspace.projects {
			clear(project.failedReads)
			project.pendingFiles, project.pendingGit = true, true
			if overview && node == r.controller.Target().String() && project.owner.Project != nil && project.owner.Project.String() == projectID {
				project.pendingGit = false
				project.readGit()
			}
		}
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
	if refresh {
		// The open settings page can belong to a different Node than Refresh.
		root := r.localRuntime()
		if mounted := root.activeFailedPluginSettingsView(); mounted != nil {
			delete(root.pluginViews.mounts, mounted.key)
		}
	}
}
