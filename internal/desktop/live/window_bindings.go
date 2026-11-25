package live

import "github.com/sailry/sailry-harness/sdk/desktop/view"

type nativeWindowResource struct {
	beforeFrame func()
	afterFrame  func()
	close       func()
}

func (r *Runtime) bindNativeWindow() {
	host := &windowViewHost{runtime: r}
	r.App.BindWindow(view.WithServices(r.ctx, host.services()))
}

func (r *Runtime) frameNativeWindowResources() {
	for _, resource := range r.nativeWindowResources {
		if resource.beforeFrame != nil {
			resource.beforeFrame()
		}
	}
}

func (r *Runtime) finishNativeWindowResources() {
	for _, resource := range r.nativeWindowResources {
		if resource.afterFrame != nil {
			resource.afterFrame()
		}
	}
}

func (r *Runtime) closeNativeWindow() {
	if r.parent != nil {
		return
	}
	if r.App != nil {
		r.App.CloseWindowBindings()
	}
	resources := r.nativeWindowResources
	r.nativeWindowResources = nil
	for _, resource := range resources {
		if resource.close != nil {
			resource.close()
		}
	}
}
