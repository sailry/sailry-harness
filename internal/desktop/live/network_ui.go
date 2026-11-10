package live

import "github.com/egoist/mygo/ui"

func (r *Runtime) initNetworkUI() {
	r.initPorts()
	r.initHosts()
	r.initConnectionActions()
	r.initSidebarNavigation()
}

func (r *Runtime) networkFrame(c *ui.Context) {
	r.connectionsFrame(c)
	r.hostsFrame(c)
	r.portsFrame()
}

func (r *Runtime) closeNetworkUI() {
	r.closeConnections()
	r.closeHosts()
	r.closePorts()
}
