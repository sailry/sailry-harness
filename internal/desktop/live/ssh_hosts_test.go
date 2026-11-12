package live

import (
	"fmt"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testSSHHostInstall covers the Add host dialog against a Node: each step is
// sent once, a refused login is a plain failure, an uncertain installation is
// checked by its receipt instead of being sent again, a secret left blank
// keeps the saved credential, and WSL installs in one step.
func testSSHHostInstall(t *testing.T) {
	// The toasts are drawn, and MyGo's raster workers cannot run inside a
	// synctest bubble, so this waits on the adapter's own workers instead.
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}
	controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	root := &Runtime{
		ctx: t.Context(), controller: controller, packages: newPackageState(),
		busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
		App:         shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
		connections: &connectionState{},
	}
	t.Cleanup(func() {
		root.workers.Wait()
		_ = controller.Close()
	})
	{
		root.initHosts()
		root.App.Settings.AddHost()
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			root.mu.Lock()
			queue := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, apply := range queue {
				apply(c)
			}
		}, 800, 600)
		// Each completion may start the next step: wait for the workers,
		// apply what they queued, and again until nothing is queued.
		settle := func() {
			t.Helper()
			for range 8 {
				root.workers.Wait()
				tester.Frame()
			}
		}
		fault := func(code protocol.ErrorCode) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: code, Message: "fixture"}}
		}
		installed := protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: protocol.NodeID{7}}}}
		// The fixture Node saves a connection at the next revision and
		// answers every other step as the case under test scripts it.
		revision := uint64(0)
		answers := map[string]link.Response{}
		var receipt protocol.RequestOutcome
		var refused *link.Response
		source.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "save_ssh":
				if refused != nil {
					return *refused
				}
				payload, _ := detail[protocol.CommandSaveSshPayload](request.Command.Data)
				if payload.ExpectedRevision != revision {
					return fault(protocol.ErrorCodeRevisionConflict)
				}
				revision++
				profile := payload.Profile
				profile.Revision = revision
				return link.Response{Output: protocol.Output{Kind: "ssh_profile", Data: profile}}
			case "inspect_request":
				payload, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
				return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: payload.ID, Outcome: receipt}}}
			}
			if answer, scripted := answers[request.Command.Kind]; scripted {
				return answer
			}
			return fault(protocol.ErrorCodeInternal)
		}
		local := root.hostState(root.controller.Target().String())
		results := []bool{}
		completed := func(ok bool) { results = append(results, ok) }

		// A process the page lists is stopped on the host that listed it,
		// by the identity it was listed with, one request at a time; a
		// process the Node always refuses is never asked for.
		stop := root.hostActions(local.view.ID).StopProcess
		answers["stop_host_process"] = link.Response{Output: protocol.Output{Kind: "host_process_signalled", Data: protocol.OutputHostProcessSignalledPayload{Pid: 41, StartedAtSecs: 9}}}
		stop(host.Process{Name: "node", PID: 41, Started: 9}, true)
		if !local.view.Stopping {
			t.Fatal("a sent stop request is not pending")
		}
		stop(host.Process{Name: "node", PID: 41, Started: 9}, true)
		stop(host.Process{Name: "launchd", PID: 1, Started: 9}, true)
		settle()
		signals := source.sent("stop_host_process")
		if len(signals) != 1 || local.view.Stopping {
			t.Fatalf("stop requests: %d sent, pending %v", len(signals), local.view.Stopping)
		}
		if signal, _ := detail[protocol.CommandStopHostProcessPayload](signals[0].Data); signal != (protocol.CommandStopHostProcessPayload{Pid: 41, StartedAtSecs: 9, Force: true}) {
			t.Fatalf("the stop request: %+v", signal)
		}
		// A refused request ends too, and the next is a request to end.
		answers["stop_host_process"] = fault(protocol.ErrorCodeConflict)
		stop(host.Process{Name: "node", PID: 41, Started: 9}, false)
		settle()
		signals = source.sent("stop_host_process")
		if signal, _ := detail[protocol.CommandStopHostProcessPayload](signals[len(signals)-1].Data); len(signals) != 2 || signal.Force || local.view.Stopping {
			t.Fatalf("a refused stop request: %d sent, %+v, pending %v", len(signals), signal, local.view.Stopping)
		}
		// The header's line names this computer's platform from its Node's
		// facts, and the running build's version and protocol after it. The
		// page has no runtime section (AGENTS.md, Removed UI), so the time zone
		// the Node reports is not projected.
		system, release, machine, zone := "darwin", "27.2", "arm64", "JST +09:00"
		info := protocol.DefaultHostInfo()
		info.Os, info.OsVersion, info.Architecture, info.Timezone = &system, &release, &machine, &zone
		local.info = &info
		root.projectHosts(root.App.Data)
		if local.view.Detail != "darwin · 27.2 · arm64" || local.view.Runtime.Version != link.ProductVersion ||
			host.Protocol(local.view.Runtime.ProtocolMin, local.view.Runtime.ProtocolMax) != fmt.Sprintf("v%d", protocol.VERSION) {
			t.Fatalf("the header's facts: %q, version %q, protocol %d-%d", local.view.Detail, local.view.Runtime.Version, local.view.Runtime.ProtocolMin, local.view.Runtime.ProtocolMax)
		}
		header := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() { host.Page(c, &host.State{}, local.view, host.Actions{}) })
		}, 1200, 1600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		header.SetLocale("en")
		header.Frame()
		header.Frame()
		line := fmt.Sprintf("darwin · 27.2 · arm64 · %s %s · %s v%d", i18n.In("en", "host_version"), link.ProductVersion, i18n.In("en", "host_protocol"), protocol.VERSION)
		if !header.HasText(line) {
			t.Fatalf("the live host's header has no %q: %q", line, header.Texts())
		}
		for _, text := range []string{"Runtime", "Host type", "Worktrees", "Time zone", zone, i18n.In("en", "host_version"), i18n.In("en", "host_protocol")} {
			// A text of its own: the header's line holds the versions' labels.
			if slices.Contains(header.Texts(), text) {
				t.Fatalf("the live host's page shows %q of the removed runtime section: %q", text, header.Texts())
			}
		}
		// Nor a projects section: neither its title, its empty state nor Add
		// project, whatever the host's projects.
		for _, text := range header.Texts() {
			if text == i18n.In("en", "projects") || text == i18n.In("en", "project_add") || strings.Contains(text, "No projects on this host") {
				t.Fatalf("the live host's page shows %q of the removed projects section: %q", text, header.Texts())
			}
		}
		if _, ok := header.AccessByRole(ui.RoleButton, i18n.In("en", "project_add")); ok {
			t.Fatal("the live host's page has an Add project button")
		}
		local.info = nil
		count := func(kind string) int { return len(source.sent(kind)) }
		project := protocol.ProjectID{1}
		draft := settings.ConnectionDraft{
			Method: settings.ByAgent, Name: "Build", Host: "build.example", Port: "2222", Username: "sail",
			Scope: settings.ScopeProjects, Projects: project.String(),
		}

		// A login the host refuses fails plainly: nothing is left to check.
		answers["check_ssh"] = fault(protocol.ErrorCodeUnavailable)
		root.addSSHHost(draft, completed)
		if !root.App.Settings.ConnectionPending {
			t.Fatal("a sent installation is not pending")
		}
		root.addSSHHost(draft, completed)
		settle()
		if count("save_ssh") != 1 || count("check_ssh") != 1 {
			t.Fatalf("a pending installation was sent again: %d saves, %d checks", count("save_ssh"), count("check_ssh"))
		}
		if len(results) != 2 || results[0] || results[1] || root.App.Settings.ConnectionPending || root.connectionStatus().Recovery {
			t.Fatalf("a refused login: results %v, pending %v, recovery %v", results, root.App.Settings.ConnectionPending, root.connectionStatus().Recovery)
		}
		save, _ := detail[protocol.CommandSaveSshPayload](source.sent("save_ssh")[0].Data)
		sharing := save.Profile.Sharing
		shared, _ := detail[[]protocol.ProjectID](sharing.Data)
		if save.Profile.Authentication != protocol.SshAuthenticationAgent || save.Credential == nil || save.Credential.Kind != "agent" ||
			save.Profile.Port != 2222 || sharing == nil || sharing.Kind != "projects" || len(shared) != 1 || shared[0] != project {
			t.Fatalf("agent connection: %+v with %+v", save.Profile, save.Credential)
		}
		if status := root.connectionStatus(); status.Saved != settings.ByAgent {
			t.Fatalf("saved method = %d", status.Saved)
		}

		// A changed draft updates the saved connection and keeps its
		// credential; an installation that may have run is then checked.
		draft.Name = "Build server"
		answers["check_ssh"] = link.Response{Output: protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "connected"}}}
		answers["install_host"] = fault(protocol.ErrorCodeOutcomeUnknown)
		root.addSSHHost(draft, completed)
		settle()
		saves := source.sent("save_ssh")
		if len(saves) != 2 {
			t.Fatalf("save_ssh commands = %d, want 2", len(saves))
		}
		update, _ := detail[protocol.CommandSaveSshPayload](saves[1].Data)
		if update.Profile.ID != save.Profile.ID || update.ExpectedRevision != 1 || update.Credential != nil {
			t.Fatalf("updated connection: id kept %v, revision %d, credential %+v", update.Profile.ID == save.Profile.ID, update.ExpectedRevision, update.Credential)
		}
		if count("install_host") != 1 || !root.connectionStatus().Recovery || root.App.Settings.ConnectionPending || results[len(results)-1] {
			t.Fatalf("uncertain installation: %d installs, recovery %v", count("install_host"), root.connectionStatus().Recovery)
		}
		other := draft
		other.Host = "other.example"
		root.addSSHHost(other, completed)
		settle()
		if count("save_ssh") != 2 || count("install_host") != 1 || !root.connectionStatus().Recovery {
			t.Fatal("an uncertain installation took another target")
		}

		// The receipt still running keeps the check; a recorded success
		// ends it without another installation.
		receipt = protocol.RequestOutcome{Kind: "admitted"}
		root.checkSSHHost(completed)
		root.checkSSHHost(completed)
		settle()
		if count("inspect_request") != 1 || !root.connectionStatus().Recovery {
			t.Fatalf("a running installation: %d inspections, recovery %v", count("inspect_request"), root.connectionStatus().Recovery)
		}
		receipt = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &installed}}
		root.checkSSHHost(completed)
		settle()
		if count("install_host") != 1 || !results[len(results)-1] || root.connectionStatus().Recovery || root.App.Settings.ConnectionPending {
			t.Fatalf("a recorded success: %d installs, results %v", count("install_host"), results)
		}

		// A recorded failure is definite; a receipt the Node lost leaves
		// the host to the user and the dialog free for a new draft.
		for _, lost := range []bool{false, true} {
			draft.Host = map[bool]string{false: "second.example", true: "third.example"}[lost]
			local.install, revision = nil, 0
			root.addSSHHost(draft, completed)
			settle()
			if !root.connectionStatus().Recovery {
				t.Fatal("an uncertain installation is not recoverable")
			}
			receipt = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture"}}}
			if lost {
				receipt = protocol.RequestOutcome{Kind: "unknown"}
			}
			root.checkSSHHost(completed)
			settle()
			if root.connectionStatus().Recovery || root.App.Settings.ConnectionPending || results[len(results)-1] {
				t.Fatalf("lost %v: the check left the dialog locked", lost)
			}
			if (local.install == nil) != lost {
				t.Fatalf("lost %v: installation kept = %v", lost, local.install != nil)
			}
		}

		// An update whose result the Node can never confirm is answered by the
		// Host itself: the operation leaves recovery, its request is given
		// up, and Check and Apply are free again. No network is bound here,
		// so the Host cannot be asked and the card offers Check.
		command, err := sshSaveCommand(draft, nil)
		if err != nil {
			t.Fatal(err)
		}
		updated := protocol.NodeID{7}
		for name, lost := range map[string]protocol.RequestOutcome{
			"recorded as unconfirmed":  {Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host update was started but reconnection could not be confirmed"}}},
			"interrupted by a restart": {Kind: "unknown"},
		} {
			request := root.controller.Prepare(command)
			local.view.Update = &host.Update{Phase: host.Staged, Retry: host.Apply}
			if local.update == nil {
				local.update = &hostUpdate{}
			}
			local.update.form = true
			update := &sshInstallation{state: local, expected: &updated, request: &request, retry: command, uncertain: true, active: true}
			local.install, receipt = update, lost
			inspections := count("inspect_request")
			root.recoverSSH(update)
			settle()
			if update.partial || update.uncertain || update.active || update.request != nil || update.retry.Kind != "" || local.update.form {
				t.Fatalf("%s: the update stayed in recovery: %+v", name, update)
			}
			if view := local.view.Update; view.Phase != host.UpdateFailed || view.Retry != host.Check {
				t.Fatalf("%s: the Host was not asked: %+v", name, view)
			}
			// Check asks the Host again instead of the request that was given up.
			root.advanceHostUpdate(local.view.ID, *local.view.Update)
			settle()
			if count("inspect_request") != inspections+1 || update.partial || update.uncertain {
				t.Fatalf("%s: Check went back to the request: %d inspections", name, count("inspect_request")-inspections)
			}
		}
		local.install = nil

		// A receipt the Node never admitted sends the same request again.
		draft.Host, local.install, revision = "fourth.example", nil, 0
		root.addSSHHost(draft, completed)
		settle()
		installs := count("install_host")
		answers["install_host"] = link.Response{Output: installed}
		receipt = protocol.RequestOutcome{Kind: "not_admitted"}
		root.checkSSHHost(completed)
		settle()
		if count("install_host") != installs+1 || !results[len(results)-1] || root.connectionStatus().Recovery {
			t.Fatalf("an unadmitted installation: %d installs, results %v", count("install_host")-installs, results)
		}

		// A save refused for a shared project that went away keeps the
		// saved connection, which the next save updates instead of saving
		// another.
		draft.Host, local.install, revision = "sixth.example", nil, 0
		answers["install_host"] = fault(protocol.ErrorCodeInternal)
		root.addSSHHost(draft, completed)
		settle()
		saves = source.sent("save_ssh")
		kept, _ := detail[protocol.CommandSaveSshPayload](saves[len(saves)-1].Data)
		missing := fault(protocol.ErrorCodeNotFound)
		refused = &missing
		draft.Name = "Shared"
		root.addSSHHost(draft, completed)
		settle()
		if status := root.connectionStatus(); status.Saved != settings.ByAgent || status.Recovery || root.App.Settings.ConnectionPending || results[len(results)-1] {
			t.Fatalf("a refused save: saved method %d, recovery %v, results %v", status.Saved, status.Recovery, results)
		}
		refused = nil
		draft.Name = "Shared again"
		root.addSSHHost(draft, completed)
		settle()
		saves = source.sent("save_ssh")
		again, _ := detail[protocol.CommandSaveSshPayload](saves[len(saves)-1].Data)
		if again.Profile.Name != "Shared again" || again.Profile.ID != kept.Profile.ID || again.ExpectedRevision != 1 || again.Credential != nil {
			t.Fatalf("after a refused save: id kept %v, revision %d, credential %+v", again.Profile.ID == kept.Profile.ID, again.ExpectedRevision, again.Credential)
		}

		// Closing the dialog abandons an installation left to check.
		draft.Host, local.install, revision = "fifth.example", nil, 0
		answers["install_host"] = fault(protocol.ErrorCodeOutcomeUnknown)
		root.addSSHHost(draft, completed)
		settle()
		if !root.connectionStatus().Recovery {
			t.Fatal("an uncertain installation is not recoverable")
		}
		root.App.Settings.CloseConnectionDialogs()
		root.sshFrame(nil, local.install)
		if local.install != nil {
			t.Fatal("a closed dialog kept its installation")
		}
		root.App.Settings.AddHost()

		// A password left blank keeps the saved one; a new one replaces it.
		saved := &protocol.SshProfile{ID: protocol.NewSshID(), Revision: 4, Authentication: protocol.SshAuthenticationPassword, HostKey: &protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:fixture"}}
		draft.Method, draft.Password = settings.ByPassword, ""
		for _, password := range []string{"", "secret"} {
			draft.Password = password
			command, err := sshSaveCommand(draft, saved)
			if err != nil {
				t.Fatal(err)
			}
			payload := command.Data.(protocol.CommandSaveSshPayload)
			if payload.Profile.ID != saved.ID || payload.Profile.HostKey != saved.HostKey || payload.ExpectedRevision != 4 || (payload.Credential == nil) != (password == "") {
				t.Fatalf("password %q: %+v with credential %v", password, payload.Profile, payload.Credential != nil)
			}
		}
		draft.Method, draft.Key = settings.ByPrivateKey, ""
		if command, err := sshSaveCommand(draft, saved); err != nil || command.Data.(protocol.CommandSaveSshPayload).Credential == nil {
			t.Fatalf("another method kept the saved credential: %v", err)
		}

		// WSL installs in one step, and its failure is reported once.
		network := protocol.NodeID{8}
		answers["install_wsl_host"] = link.Response{Output: protocol.Output{Kind: "host_installation", Data: protocol.HostInstallationOutcome{Kind: "installed", Data: protocol.HostInstallationOutcomeInstalledPayload{Node: network}}}}
		local.install = nil
		root.addSSHHost(settings.ConnectionDraft{Method: settings.ByWSL, Distribution: "Ubuntu"}, completed)
		settle()
		wsl := source.sent("install_wsl_host")
		if len(wsl) != 1 || !results[len(results)-1] || root.App.Settings.ConnectionPending {
			t.Fatalf("WSL installation: %d commands, results %v", len(wsl), results)
		}
		if payload, _ := detail[protocol.CommandInstallWslHostPayload](wsl[0].Data); payload.Distribution != "Ubuntu" || payload.Network == nil || payload.Network.Kind != "iroh" {
			t.Fatalf("WSL installation payload: %+v", payload)
		}

		// Only WSL2 distributions are offered.
		answers["list_wsl_distributions"] = link.Response{Output: protocol.Output{Kind: "wsl_distributions", Data: []protocol.WslDistribution{{Name: "Legacy", Version: 1}, {Name: "Ubuntu", Version: 2}}}}
		root.loadWSLDistributions()
		if !root.connectionStatus().DistributionsLoading {
			t.Fatal("loading distributions is not shown")
		}
		settle()
		if status := root.connectionStatus(); status.DistributionsLoading || !status.DistributionsLoaded || len(status.Distributions) != 1 || status.Distributions[0] != "Ubuntu" {
			t.Fatalf("WSL distributions: %+v", status)
		}
	}
}
