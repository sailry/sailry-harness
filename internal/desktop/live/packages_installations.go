package live

import (
	"errors"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) observeInstallations() {
	state := r.packages
	state.observing = true
	r.observe(func() {
		stream, err := r.controller.Subscribe(r.ctx, protocol.Topic{Kind: "installations"})
		if err != nil {
			r.enqueue(func(*ui.Context) { state.connected = false })
			return
		}
		defer stream.Close()
		for {
			update, err := stream.Next(r.ctx)
			if err != nil {
				r.enqueue(func(*ui.Context) { state.connected = false })
				return
			}
			value, valid := detail[protocol.UpdateInstallationsPayload](update.Data)
			if !valid || update.Kind != "installations" || value.Node != r.controller.Target() {
				continue
			}
			r.enqueue(func(*ui.Context) { state.connected, state.tasks = true, value.Tasks })
		}
	})
}

func installationEntry(node string, task protocol.InstallationTask, connected bool) transfers.Entry {
	entry := transfers.Entry{Key: node + "/" + task.Request.String(), Kind: transfers.Plugin, Connected: connected, Percent: -1}
	switch task.Resource.Kind {
	case "plugin":
		if value, valid := detail[protocol.InstallationResourcePluginPayload](task.Resource.Data); valid {
			entry.Title = value.Name
		}
	case "model":
		entry.Kind = transfers.Model
		if value, valid := detail[protocol.InstallationResourceModelPayload](task.Resource.Data); valid {
			entry.Title = value.ID
		}
	case "language":
		entry.Kind = transfers.Language
		if value, valid := detail[protocol.InstallationResourceLanguagePayload](task.Resource.Data); valid {
			entry.Title = value.Package + " " + value.Version
		}
	}
	switch task.State.Kind {
	case "running":
		entry.Status = transfers.Running
		if value, valid := detail[protocol.InstallationStateRunningPayload](task.State.Data); valid {
			entry.Phase = "installation_" + string(value.Phase)
			if value.Progress != nil {
				switch value.Progress.Kind {
				case "percent":
					if progress, valid := detail[protocol.InstallationProgressPercentPayload](value.Progress.Data); valid {
						entry.Percent = float64(progress.Percent)
						if progress.BytesPerSecond != nil {
							entry.Speed = float64(*progress.BytesPerSecond)
						}
					}
				case "counts":
					if progress, valid := detail[protocol.InstallationProgressCountsPayload](value.Progress.Data); valid {
						entry.Completed, entry.Total = int(progress.Completed), int(progress.Total)
						if progress.BytesPerSecond != nil {
							entry.Speed = float64(*progress.BytesPerSecond)
						}
					}
				}
			}
		}
	case "failed":
		entry.Status, entry.Actionable = transfers.Failed, true
		if value, valid := detail[protocol.InstallationStateFailedPayload](task.State.Data); valid {
			entry.Error = value.Error.Message
		}
	case "uncertain":
		entry.Status, entry.Actionable = transfers.Uncertain, true
		if value, valid := detail[protocol.InstallationStateUncertainPayload](task.State.Data); valid {
			entry.Error = value.Error.Message
		}
	default:
		entry.Status = transfers.Queued
	}
	return entry
}

func (r *Runtime) projectInstallations() {
	root := r.localRuntime()
	if root.packages == nil {
		return
	}
	queue := &root.packages.queue
	queue.Entries = nil
	owners := []*Runtime{root}
	for _, owner := range root.bindings {
		owners = append(owners, owner)
	}
	for _, owner := range owners {
		if owner.packages == nil {
			continue
		}
		for _, task := range owner.packages.tasks {
			entry := installationEntry(owner.controller.Target().String(), task, owner.packages.connected && owner.controller.Connected())
			_, available := owner.packages.requests[task.Request.String()]
			entry.Retryable = entry.Actionable && available
			queue.Entries = append(queue.Entries, entry)
		}
	}
	queue.Dismiss = root.dismissInstallation
	queue.RetryAction = root.retryInstallation
}

func (r *Runtime) installationOwner(entry transfers.Entry) (*Runtime, protocol.RequestID, bool) {
	node, text, valid := strings.Cut(entry.Key, "/")
	id, err := protocol.ParseRequestID(text)
	owner := r.runtimeFor(node)
	return owner, id, valid && err == nil && owner != nil && owner.packages != nil
}

func (r *Runtime) dismissInstallation(entry transfers.Entry) {
	owner, id, valid := r.installationOwner(entry)
	if !valid {
		return
	}
	owner.packageCommand(protocol.Command{Kind: "dismiss_installation", Data: protocol.CommandDismissInstallationPayload{Request: id}}, func(_ protocol.Output, ok bool) {
		if ok {
			var tasks []protocol.InstallationTask
			for _, task := range owner.packages.tasks {
				if task.Request != id {
					tasks = append(tasks, task)
				}
			}
			owner.packages.tasks = tasks
			delete(owner.packages.requests, id.String())
		}
	})
}

func (r *Runtime) retryInstallation(entry transfers.Entry) {
	owner, id, valid := r.installationOwner(entry)
	if !valid {
		return
	}
	request, exists := owner.packages.requests[id.String()]
	if !exists {
		return
	}
	if entry.Status == transfers.Failed {
		owner.packageCommand(protocol.Command{Kind: "dismiss_installation", Data: protocol.CommandDismissInstallationPayload{Request: id}}, func(_ protocol.Output, ok bool) {
			if ok {
				owner.packageCommand(request.Command, nil)
			}
		})
		return
	}
	owner.async(func() {
		out, err := owner.retryAttachmentRequest(owner.ctx, request)
		owner.enqueue(func(c *ui.Context) {
			owner.packageReport(c, err)
			if err == nil {
				if info, valid := detail[protocol.PluginInfo](out.Data); valid {
					owner.packages.infos[info.Summary.Name] = info
				}
				owner.dismissInstallation(entry)
			} else if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
				delete(owner.packages.pending, packageCommandKey(request.Command))
				for index := range owner.packages.tasks {
					if owner.packages.tasks[index].Request == id {
						fault := protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: err.Error()}
						var typed *link.FaultError
						if errors.As(err, &typed) {
							fault = typed.Fault
						}
						owner.packages.tasks[index].State = protocol.InstallationState{Kind: "failed", Data: protocol.InstallationStateFailedPayload{Error: fault}}
					}
				}
			}
		})
	})
}
