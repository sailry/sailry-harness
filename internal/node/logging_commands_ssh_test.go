package node

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/json"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/getsentry/sentry-go"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

const (
	sshLogName      = "ssh-canary-profile"
	sshLogUser      = "ssh-canary-user"
	sshLogPassword  = "ssh-canary-password"
	sshLogBanner    = "SSH-2.0-ssh-canary-banner"
	sshLogHome      = "/home/ssh-canary-remote"
	sshLogStdout    = "ssh-canary-stdout"
	sshLogStderr    = "ssh-canary-stderr"
	sshLogHandshake = "handshake"
	sshLogPlatform  = "uname -s; uname -m"
	sshLogPaths     = "printf '%s\\n' \"$HOME\"; uname -s; id -u"
	sshLogInstall   = "sh -s"
)

type sshLogPeer struct {
	address       *net.TCPAddr
	key           protocol.SshHostKey
	allowPassword atomic.Bool
	passwordSeen  atomic.Bool
	installerRuns atomic.Int32
	pause         atomic.Pointer[sshLogPause]
}

type sshLogPause struct {
	stage   string
	reached chan struct{}
	resume  chan struct{}
}

func newSSHLogPeer(t *testing.T) *sshLogPeer {
	t.Helper()
	signer, err := cryptossh.NewSignerFromKey(ed25519.NewKeyFromSeed(make([]byte, ed25519.SeedSize)))
	if err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	peer := &sshLogPeer{
		address: listener.Addr().(*net.TCPAddr),
		key:     protocol.SshHostKey{Algorithm: signer.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(signer.PublicKey())},
	}
	config := &cryptossh.ServerConfig{ServerVersion: sshLogBanner, PasswordCallback: func(conn cryptossh.ConnMetadata, password []byte) (*cryptossh.Permissions, error) {
		if conn.User() == sshLogUser && string(password) == sshLogPassword {
			peer.passwordSeen.Store(true)
			if peer.allowPassword.Load() {
				return nil, nil
			}
		}
		return nil, errors.New("fixture rejected password")
	}}
	config.AddHostKey(signer)
	done := make(chan struct{})
	t.Cleanup(func() {
		_ = listener.Close()
		select {
		case <-done:
		case <-time.After(10 * time.Second):
			t.Error("SSH log fixture did not stop")
		}
	})
	go func() {
		defer close(done)
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			peer.serve(conn, config)
		}
	}()
	return peer
}

func (peer *sshLogPeer) pauseAt(stage string) *sshLogPause {
	pause := &sshLogPause{stage: stage, reached: make(chan struct{}), resume: make(chan struct{})}
	peer.pause.Store(pause)
	return pause
}

func (peer *sshLogPeer) arrive(stage string) *sshLogPause {
	pause := peer.pause.Load()
	if pause == nil || pause.stage != stage {
		return nil
	}
	peer.pause.Store(nil)
	close(pause.reached)
	return pause
}

func (pause *sshLogPause) wait(t *testing.T, ctx context.Context) {
	t.Helper()
	select {
	case <-pause.reached:
	case <-ctx.Done():
		t.Fatalf("SSH did not reach %q: %v", pause.stage, ctx.Err())
	}
}

func (peer *sshLogPeer) serve(conn net.Conn, config *cryptossh.ServerConfig) {
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(10 * time.Second))
	if pause := peer.pause.Load(); pause != nil && pause.stage == sshLogHandshake {
		var versionPrefix [4]byte
		if _, err := io.ReadFull(conn, versionPrefix[:]); err != nil || string(versionPrefix[:]) != "SSH-" {
			return
		}
		peer.arrive(sshLogHandshake)
		_, _ = io.Copy(io.Discard, conn)
		return
	}
	server, channels, global, err := cryptossh.NewServerConn(conn, config)
	if err != nil {
		return
	}
	defer server.Close()
	go cryptossh.DiscardRequests(global)
	for offered := range channels {
		if offered.ChannelType() != "session" {
			_ = offered.Reject(cryptossh.UnknownChannelType, "session expected")
			continue
		}
		channel, requests, err := offered.Accept()
		if err != nil {
			return
		}
		peer.exec(channel, requests)
	}
}

func (peer *sshLogPeer) exec(channel cryptossh.Channel, requests <-chan *cryptossh.Request) {
	defer channel.Close()
	request, ok := <-requests
	if !ok {
		return
	}
	var payload struct{ Command string }
	if request.Type != "exec" || cryptossh.Unmarshal(request.Payload, &payload) != nil {
		_ = request.Reply(false, nil)
		return
	}
	var stdout, stderr string
	var status uint32
	switch payload.Command {
	case sshLogPlatform:
		stdout = "Linux\nx86_64\n"
	case sshLogPaths:
		stdout = sshLogHome + "\nLinux\n1000\n"
	case sshLogInstall:
		stdout, stderr, status = sshLogStdout, sshLogStderr+" "+sshLogHome+"\n", 1
	default:
		_ = request.Reply(false, nil)
		return
	}
	if request.Reply(true, nil) != nil {
		return
	}
	if payload.Command == sshLogInstall {
		if size, err := io.Copy(io.Discard, channel); err != nil || size == 0 {
			return
		}
		peer.installerRuns.Add(1)
	}
	if pause := peer.arrive(payload.Command); pause != nil {
		select {
		case <-pause.resume:
		case <-requests:
			return
		}
	}
	if _, err := io.WriteString(channel, stdout); err != nil {
		return
	}
	if _, err := io.WriteString(channel.Stderr(), stderr); err != nil {
		return
	}
	_, _ = channel.SendRequest("exit-status", false, cryptossh.Marshal(struct{ Status uint32 }{status}))
}

func TestSSHDispatchPrivacy(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	const profileDir = "ssh-canary-local-profile"
	path := filepath.Join(root, profileDir)
	closeLog, err := logging.Install(path, "desktop")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	artifactRoot := host.ArtifactRoot
	host.ArtifactRoot = t.TempDir()
	t.Cleanup(func() { host.ArtifactRoot = artifactRoot })
	peer := newSSHLogPeer(t)
	server := newSentryFixture(t)
	node, err := StartWithOptions(t.Context(), path, Options{
		Environment: new(process.NewEnvironment(nil)), MCP: fixtureMCPOptions(),
		Network: &link.NetworkScope{Bind: "127.0.0.1:0"},
		Telemetry: telemetry.Options{
			Lookup:    environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn()}),
			Transport: server.server.Client().Transport,
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	}()
	ctx, stop := context.WithTimeout(t.Context(), 25*time.Second)
	defer stop()
	controller := client.New(link.NewLocal(node.ID(), node.ID(), node))
	defer controller.Close()
	if !node.logs || !node.telemetry.Errors() {
		t.Fatal("fixture requires local logs and automatic error reports")
	}

	newRequest := func(kind string, data any) protocol.Request {
		return protocol.NewRequest(node.ID(), protocol.Command{Kind: kind, Data: data})
	}
	admit := func(ctx context.Context, request protocol.Request) link.Admission {
		t.Helper()
		admitted, err := node.Dispatch(ctx, node.ID(), request)
		if err != nil || !admitted.Receipt.Durable || admitted.Receipt.ID != request.ID || admitted.Completion == nil {
			t.Fatalf("%s receipt = %+v, error = %v", request.Command.Kind, admitted.Receipt, err)
		}
		return admitted
	}
	await := func(admitted link.Admission) link.Response {
		t.Helper()
		select {
		case result, ok := <-admitted.Completion:
			if !ok {
				t.Fatal("missing SSH completion")
			}
			return result
		case <-ctx.Done():
			t.Fatalf("SSH request %s: %v", admitted.Receipt.ID, ctx.Err())
			return link.Response{}
		}
	}
	var outcomes []sshLogOutcome
	failure := func(request protocol.Request, fault *protocol.Fault, commandStage string, code protocol.ErrorCode, reason protocol.SSHFailureReason) {
		t.Helper()
		if fault == nil || fault.Code != code || fault.SSHReason != reason {
			t.Fatalf("%s %s fault = %+v, want %s / %s", request.Command.Kind, commandStage, fault, code, reason)
		}
		phase, level := logging.Failed, "ERROR"
		if code == protocol.ErrorCodeCancelled {
			phase, level = logging.Cancelled, "INFO"
			if fault.ReportRef != "" {
				t.Fatal("cancelled SSH operation has a report reference")
			}
		} else if !logging.ValidReportRef(fault.ReportRef) {
			t.Fatal("SSH refusal has no valid report reference")
		}
		outcomes = append(outcomes, sshLogOutcome{request: request, ref: fault.ReportRef, level: level, fields: map[string]string{
			"area": "command", "operation": request.Command.Kind, "phase": phase, "code": string(code),
			"command_stage": commandStage, "stage": reason.Stage(), "reason": string(reason),
		}})
	}
	storedFailure := func(request protocol.Request, delivered *protocol.Fault, state client.Settled) {
		t.Helper()
		stored, err := controller.Resolve(ctx, request)
		if err != nil || stored.State != state || stored.Fault == nil || stored.Fault.ReportRef != "" ||
			stored.Fault.Code != delivered.Code || stored.Fault.Message != delivered.Message || stored.Fault.SSHReason != delivered.SSHReason {
			t.Fatalf("%s stored result = %+v, error = %v", request.Command.Kind, stored, err)
		}
	}
	profile := protocol.SshProfile{
		ID: protocol.NewSshID(), Name: sshLogName, Host: peer.address.IP.String(), Port: uint16(peer.address.Port),
		Username: sshLogUser, Authentication: protocol.SshAuthenticationPassword,
	}
	credential := protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(sshLogPassword)}}
	saved := commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{Profile: profile, Credential: &credential}})
	profile, ok := dispatchPayload[protocol.SshProfile](saved.Data)
	if saved.Kind != "ssh_profile" || !ok {
		t.Fatalf("save_ssh output = %s", saved.Kind)
	}
	check := func() protocol.Request {
		return newRequest("check_ssh", protocol.CommandCheckSshPayload{Profile: profile.ID, ExpectedRevision: profile.Revision})
	}
	install := func() protocol.Request {
		return newRequest("install_host", protocol.CommandInstallHostPayload{Profile: profile.ID, ExpectedRevision: profile.Revision})
	}
	for _, request := range []protocol.Request{check(), install()} {
		request.Version++
		admitted, err := node.Dispatch(ctx, node.ID(), request)
		var fault *protocol.Fault
		if !errors.As(err, &fault) || admitted.Receipt.Durable || admitted.Completion != nil {
			t.Fatalf("%s admission with mismatched version = %v", request.Command.Kind, err)
		}
		failure(request, fault, "admission", protocol.ErrorCodeWrongTarget, "")
	}

	for _, request := range []protocol.Request{check(), install()} {
		result := await(admit(ctx, request))
		outcome, ok := dispatchPayload[protocol.SshOutcome](result.Output.Data)
		required, keyOK := dispatchPayload[protocol.SshOutcomeHostKeyRequiredPayload](outcome.Data)
		if result.Fault != nil || result.Output.Kind != "ssh_outcome" || !ok || outcome.Kind != "host_key_required" || !keyOK || required.Changed || required.Key != peer.key {
			t.Fatalf("%s did not return the loopback host key for confirmation: %+v", request.Command.Kind, result)
		}
		outcomes = append(outcomes, sshLogOutcome{request: request, level: "INFO", fields: map[string]string{
			"area": "command", "operation": request.Command.Kind, "phase": logging.Completed,
		}})
	}
	if peer.passwordSeen.Load() || peer.installerRuns.Load() != 0 {
		t.Fatal("SSH used the unconfirmed host key")
	}
	confirmed := commandFixture(t, node, protocol.Command{Kind: "trust_ssh", Data: protocol.CommandTrustSshPayload{
		Profile: profile.ID, ExpectedRevision: profile.Revision, Key: peer.key,
	}})
	trusted, ok := dispatchPayload[protocol.SshProfile](confirmed.Data)
	if confirmed.Kind != "ssh_profile" || !ok || trusted.Revision != profile.Revision+1 || trusted.HostKey == nil || *trusted.HostKey != peer.key {
		t.Fatal("trust_ssh did not confirm the host key at a new revision")
	}
	profile = trusted
	request := check()
	authFault := await(admit(ctx, request)).Fault
	failure(request, authFault, "completion", protocol.ErrorCodeUnavailable, protocol.SSHAuthFailed)
	storedFailure(request, authFault, client.RequestFailed)
	if !peer.passwordSeen.Load() {
		t.Fatal("SSH fixture did not receive the password canary")
	}
	peer.allowPassword.Store(true)

	request = install()
	pause := peer.pauseAt(sshLogPlatform)
	observer, cancelObserver := context.WithCancel(ctx)
	defer cancelObserver()
	admitted := admit(observer, request)
	pause.wait(t, ctx)
	cancelObserver()
	close(pause.resume)
	first := await(admitted).Fault
	failure(request, first, "completion", protocol.ErrorCodeUnavailable, protocol.SSHCommandNonzeroExit)
	storedFailure(request, first, client.RequestFailed)
	second := await(admit(ctx, request)).Fault
	failure(request, second, "completion", protocol.ErrorCodeUnavailable, protocol.SSHCommandNonzeroExit)
	if first.Message != "Host installation command failed" || second.Message != first.Message || second.ReportRef == first.ReportRef || peer.installerRuns.Load() != 1 {
		t.Fatal("installer failure changed or re-executed on replay")
	}

	for _, test := range []struct {
		stage       string
		makeRequest func() protocol.Request
		code        protocol.ErrorCode
		reason      protocol.SSHFailureReason
		state       client.Settled
	}{
		{sshLogHandshake, check, protocol.ErrorCodeCancelled, "", client.RequestFailed},
		{sshLogPlatform, install, protocol.ErrorCodeCancelled, "", client.RequestFailed},
		{sshLogPaths, install, protocol.ErrorCodeCancelled, "", client.RequestFailed},
		{sshLogInstall, install, protocol.ErrorCodeOutcomeUnknown, protocol.SSHCommandInterrupted, client.RequestUnknown},
	} {
		pause := peer.pauseAt(test.stage)
		request := test.makeRequest()
		admitted := admit(ctx, request)
		pause.wait(t, ctx)
		cancel := newRequest("cancel_ssh", protocol.CommandCancelSshPayload{Request: request.ID})
		cancelled := await(admit(ctx, cancel))
		ack, ok := dispatchPayload[protocol.OutputSshCancelledPayload](cancelled.Output.Data)
		if cancelled.Fault != nil || cancelled.Output.Kind != "ssh_cancelled" || !ok || ack.Request != request.ID {
			t.Fatalf("cancel_ssh = %+v", cancelled)
		}
		fault := await(admitted).Fault
		failure(request, fault, "completion", test.code, test.reason)
		storedFailure(request, fault, test.state)
	}

	if peer.installerRuns.Load() != 2 {
		t.Fatal("expected one failed installation and one interrupted installation")
	}
	private := []string{sshLogName, sshLogUser, sshLogPassword, sshLogBanner, sshLogHome, sshLogStdout, sshLogStderr,
		peer.address.String(), peer.key.Fingerprint, profile.ID.String(), profileDir}
	logPath, err := logging.CurrentFile()
	if err != nil {
		t.Fatal(err)
	}
	logBody, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	assertSSHLogOutcomes(t, logBody, node.ID(), outcomes)
	assertSSHLogPrivate(t, "local log", logBody, private)
	private = append(private, node.ID().String())
	for _, outcome := range outcomes {
		private = append(private, outcome.request.ID.String())
	}
	assertSSHReports(t, ctx, node, server, outcomes, private)
}

type sshLogOutcome struct {
	request protocol.Request
	ref     string
	level   string
	fields  map[string]string
}

func assertSSHLogPrivate(t *testing.T, surface string, body []byte, private []string) {
	t.Helper()
	for index, value := range private {
		if bytes.Contains(body, []byte(value)) {
			t.Errorf("%s carries private canary %d", surface, index)
		}
	}
}

func assertSSHLogOutcomes(t *testing.T, logBody []byte, node protocol.NodeID, outcomes []sshLogOutcome) {
	t.Helper()
	for _, outcome := range outcomes {
		matches := 0
		requestField := "request=" + outcome.request.ID.String()
		for line := range strings.SplitSeq(string(logBody), "\n") {
			fields := strings.Fields(line)
			if outcome.ref != "" {
				if !slices.Contains(fields, "report_ref="+outcome.ref) {
					continue
				}
			} else if !slices.Contains(fields, requestField) || !slices.Contains(fields, "phase="+outcome.fields["phase"]) {
				continue
			}
			matches++
			if len(fields) < 3 || fields[1] != outcome.level || fields[2] != "desktop" {
				t.Errorf("local SSH record has an unexpected level or application for %s", outcome.request.ID)
			}
			for _, field := range []string{requestField, "node=" + node.String(), "caller=" + node.String(), "command=" + outcome.request.Command.Kind} {
				if !slices.Contains(fields, field) {
					t.Errorf("local SSH record lacks %s", field)
				}
			}
			if outcome.ref == "" && strings.Contains(line, " report_ref=") {
				t.Error("local SSH record carries an unexpected reference")
			}
			for key, value := range outcome.fields {
				if value != "" && !slices.Contains(fields, key+"="+value) || value == "" && strings.Contains(line, " "+key+"=") {
					t.Errorf("local %s record has unexpected %s", outcome.request.Command.Kind, key)
				}
			}
		}
		if matches != 1 {
			t.Errorf("local %s %s records for %s = %d, want one", outcome.request.Command.Kind, outcome.fields["phase"], outcome.request.ID, matches)
		}
	}
}

func assertSSHReports(t *testing.T, ctx context.Context, node *Node, server *sentryFixture, outcomes []sshLogOutcome, private []string) {
	t.Helper()
	wanted := make(map[string]sshLogOutcome)
	for _, outcome := range outcomes {
		if outcome.ref == "" {
			continue
		}
		if _, exists := wanted[outcome.ref]; exists {
			t.Fatal("duplicate report reference across SSH deliveries")
		}
		wanted[outcome.ref] = outcome
	}
	snapshot := func() [][]byte {
		server.mu.Lock()
		defer server.mu.Unlock()
		return slices.Clone(server.bodies)
	}
	parse := func(body []byte) (sentry.Event, [][]byte) {
		t.Helper()
		lines := bytes.SplitN(body, []byte("\n"), 5)
		var event sentry.Event
		if len(lines) < 3 || json.Unmarshal(lines[2], &event) != nil {
			t.Fatal("invalid SSH Sentry envelope")
		}
		return event, lines
	}
	for {
		seen := make(map[string]bool)
		for _, body := range snapshot() {
			event, _ := parse(body)
			if ref, ok := event.Contexts["sailry"]["report_ref"].(string); ok {
				if _, expected := wanted[ref]; expected {
					seen[ref] = true
				}
			}
		}
		if len(seen) == len(wanted) {
			break
		}
		select {
		case <-ctx.Done():
			t.Fatalf("received %d of %d SSH error reports: %v", len(seen), len(wanted), ctx.Err())
		case <-time.After(10 * time.Millisecond):
		}
	}
	if out := commandFixture(t, node, protocol.Command{Kind: "send_diagnostics"}); out.Kind != "diagnostics_sent" {
		t.Fatalf("send_diagnostics = %s", out.Kind)
	}
	if err := node.Stop(ctx); err != nil {
		t.Fatal(err)
	}

	manual := 0
	for _, body := range snapshot() {
		assertSSHLogPrivate(t, "Sentry envelope", body, private)
		event, lines := parse(body)
		if event.Message == "runtime.diagnostics:requested" {
			manual++
			assertSSHAttachment(t, lines, outcomes)
			continue
		}
		fields := event.Contexts["sailry"]
		if fields["operation"] != "check_ssh" && fields["operation"] != "install_host" {
			continue
		}
		ref, _ := fields["report_ref"].(string)
		outcome, found := wanted[ref]
		if !found || !outcome.matches(fields) || event.Level != sentry.LevelError {
			t.Fatal("automatic SSH event does not match a failure delivery")
		}
		outcome.checkPublicFields(t, fields, false)
		delete(wanted, ref)
	}
	if manual != 1 || len(wanted) != 0 {
		t.Fatalf("manual diagnostic reports = %d, undelivered SSH failures = %d", manual, len(wanted))
	}
}

func (outcome sshLogOutcome) matches(fields map[string]any) bool {
	for key, want := range outcome.fields {
		got, _ := fields[key].(string)
		if got != want {
			return false
		}
	}
	return true
}

func (outcome sshLogOutcome) checkPublicFields(t *testing.T, fields map[string]any, attachment bool) {
	t.Helper()
	for key := range fields {
		if outcome.fields[key] != "" || key == "duration_us" || attachment && (key == "level" || key == "timestamp") || !attachment && key == "report_ref" {
			continue
		}
		t.Errorf("public SSH record carries unexpected field %q", key)
	}
}

func assertSSHAttachment(t *testing.T, envelope [][]byte, outcomes []sshLogOutcome) {
	t.Helper()
	var item struct {
		Type, Filename string
		ContentType    string `json:"content_type"`
		Length         int
	}
	if len(envelope) != 5 || json.Unmarshal(envelope[3], &item) != nil || item.Type != "attachment" ||
		item.Filename != "diagnostics.jsonl" || item.ContentType != "application/x-ndjson" || item.Length <= 0 ||
		item.Length != len(envelope[4])-1 || envelope[4][item.Length] != '\n' {
		t.Fatal("missing diagnostics.jsonl attachment in manual envelope")
	}
	public := envelope[4][:item.Length]
	for _, outcome := range outcomes {
		if outcome.ref != "" && bytes.Contains(public, []byte(outcome.ref)) {
			t.Error("manual attachment carries an SSH report reference")
		}
	}
	if bytes.Contains(public, []byte(`"report_ref"`)) {
		t.Error("manual attachment carries a report_ref field")
	}
	pending := slices.Clone(outcomes)
	for line := range bytes.SplitSeq(bytes.TrimSpace(public), []byte("\n")) {
		var fields map[string]any
		if err := json.Unmarshal(line, &fields); err != nil {
			t.Fatal("invalid JSON line in diagnostics.jsonl")
		}
		if (fields["operation"] != "check_ssh" && fields["operation"] != "install_host") || fields["phase"] == logging.Begin || fields["phase"] == logging.Admitted {
			continue
		}
		index := slices.IndexFunc(pending, func(outcome sshLogOutcome) bool { return outcome.matches(fields) })
		if index < 0 {
			t.Fatal("manual attachment has an unexpected SSH result")
		}
		if fields["level"] != pending[index].level {
			t.Error("manual attachment has an unexpected SSH record level")
		}
		pending[index].checkPublicFields(t, fields, true)
		pending = slices.Delete(pending, index, index+1)
	}
	if len(pending) != 0 {
		t.Fatalf("manual attachment is missing %d SSH results", len(pending))
	}
}
