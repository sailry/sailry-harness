package host

import (
	"context"
	"crypto/sha256"
	"embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/wsl"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/shipped"
	cryptossh "golang.org/x/crypto/ssh"
	"golang.org/x/mod/semver"
)

var ArtifactRoot string

//go:embed deployment/release.sh deployment/service.sh deployment/identity.py
var deploymentSources embed.FS

var stagingPath = regexp.MustCompile(`^/tmp/sailry-bootstrap-[A-Za-z0-9]+$`)

type artifact struct {
	file   *os.File
	digest string
	name   string
	size   int64
}

type installedIdentity struct {
	Node    protocol.NodeID `json:"node"`
	Version string          `json:"version"`
}

func artifactDirectory(target string) (string, error) {
	root := ArtifactRoot
	if root == "" {
		executable, err := os.Executable()
		if err != nil {
			return "", unavailable("Host build artifacts are unavailable")
		}
		root = filepath.Join(filepath.Dir(executable), "host-artifacts")
	}
	return filepath.Join(root, target), nil
}

func openArtifact(root, name string) (*artifact, error) {
	path := filepath.Join(root, name)
	metadata, err := os.Lstat(path)
	if err != nil || !metadata.Mode().IsRegular() || metadata.Size() == 0 {
		return nil, unavailable("locally built Host artifacts are unavailable for this target")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, unavailable("locally built Host artifact could not be read")
	}
	digest := sha256.New()
	size, err := io.Copy(digest, file)
	if err == nil {
		_, err = file.Seek(0, io.SeekStart)
	}
	if err != nil {
		_ = file.Close()
		return nil, unavailable("locally built Host artifact could not be verified")
	}
	return &artifact{file: file, digest: hex.EncodeToString(digest.Sum(nil)), name: name, size: size}, nil
}

func targetPlatform(platform string) (string, string, error) {
	switch strings.Join(strings.Fields(platform), "/") {
	case "Linux/x86_64":
		return "linux-amd64", "libturso_sync_sdk_kit.so", nil
	case "Linux/aarch64", "Linux/arm64":
		return "linux-arm64", "libturso_sync_sdk_kit.so", nil
	case "Darwin/arm64":
		return "darwin-arm64", "libturso_sync_sdk_kit.dylib", nil
	case "Darwin/x86_64":
		return "darwin-amd64", "libturso_sync_sdk_kit.dylib", nil
	}
	if windowsPlatform(platform) {
		return "", "", unavailable(protocol.SSHWindowsDesktopRequired)
	}
	return "", "", invalid("Host installation supports Linux and macOS on amd64 or arm64")
}

func windowsPlatform(platform string) bool {
	fields := strings.Fields(platform)
	if len(fields) == 0 {
		return false
	}
	system := strings.ToUpper(fields[0])
	return system == "WINDOWS" || strings.HasPrefix(system, "MINGW") || strings.HasPrefix(system, "MSYS") || strings.HasPrefix(system, "CYGWIN")
}

// detectPlatform accepts a POSIX target first, including WSL behind a
// Windows SSH server. Native Windows shells may not have uname; the server
// banner or a read-only cmd probe identifies them before installation starts.
func detectPlatform(ctx context.Context, connection *cryptossh.Client) (string, string, error) {
	platform, err := commandSSH(ctx, connection, sshProbe, "uname -s; uname -m", nil)
	if err == nil {
		var target, turso string
		target, turso, err = targetPlatform(platform)
		if err == nil || windowsPlatform(platform) {
			return target, turso, err
		}
	}
	if ctx.Err() != nil {
		return "", "", err
	}
	if strings.Contains(string(connection.ServerVersion()), "OpenSSH_for_Windows") {
		return "", "", unavailable(protocol.SSHWindowsDesktopRequired)
	}
	// ver is a cmd builtin, so invoke cmd explicitly even when the SSH
	// account's configured shell is PowerShell.
	version, probeErr := commandSSH(ctx, connection, sshProbe, "cmd.exe /d /c ver", nil)
	if probeErr == nil && strings.Contains(version, "Microsoft Windows") {
		return "", "", unavailable(protocol.SSHWindowsDesktopRequired)
	}
	return "", "", err
}

func validateUpdate(identity installedIdentity, node protocol.NodeID, target string) error {
	if identity.Node != node {
		return &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "SSH endpoint belongs to another Host"}
	}
	current := strings.TrimPrefix(identity.Version, officialVersionPrefix)
	if current == identity.Version || !semver.IsValid("v"+current) || !semver.IsValid("v"+target) {
		return unavailable("installed Host version could not be verified")
	}
	if semver.Compare("v"+current, "v"+target) > 0 {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Host is newer; update the controller instead"}
	}
	return nil
}

func sameRelease(current *string, target string) bool {
	return current != nil && semver.IsValid("v"+*current) && semver.IsValid("v"+target) && semver.Compare("v"+*current, "v"+target) == 0
}

const officialVersionPrefix = identity.OfficialHostVersionTag

func (owner *Installations) Execute(ctx context.Context, connection *cryptossh.Client, endpoint *link.Link, caller protocol.NodeID, request protocol.RequestID, expected *protocol.NodeID, selection *protocol.NetworkSelection) (protocol.SshOutcome, error) {
	ctx, cancel := context.WithTimeout(ctx, 600*time.Second)
	defer cancel()
	owner.Progress(caller, request, "detecting")
	target, turso, err := detectPlatform(ctx, connection)
	if err != nil {
		return protocol.SshOutcome{}, installPreflightFailure(ctx, err)
	}
	paths, err := owner.remotePaths(ctx, connection)
	if err != nil {
		return protocol.SshOutcome{}, installPreflightFailure(ctx, err)
	}
	version := link.ProductVersion
	checkRelease := owner.options.Release.Published()
	// Explicit official Host selection always uses its published repository.
	var root string
	published := checkRelease
	if !published {
		root, err = artifactDirectory(target)
		if err != nil {
			return protocol.SshOutcome{}, err
		}
		_, missing := os.Lstat(filepath.Join(root, "sailry-host"))
		published = errors.Is(missing, fs.ErrNotExist)
	}
	if published {
		version, err = wsl.ResolveRelease(ctx, owner.options.Release)
		if err != nil {
			return protocol.SshOutcome{}, installPreflightFailure(ctx, err)
		}
		err = owner.installRelease(ctx, connection, caller, request, expected, selection, version)
	} else {
		err = owner.installLocal(ctx, connection, caller, request, root, target, turso, paths, expected, selection)
	}
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	owner.Progress(caller, request, "pairing")
	if expected != nil {
		if err := confirmUpdate(ctx, endpoint, *expected, version); err != nil {
			return protocol.SshOutcome{}, err
		}
		return protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: *expected}}, nil
	}
	if !checkRelease {
		version = ""
	}
	address, err := pairInstalled(ctx, connection, endpoint, paths.Profile, version)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	if selection != nil {
		if err := ConfigureNetwork(ctx, endpoint, address, request, *selection); err != nil {
			return protocol.SshOutcome{Kind: "host_network_unconfirmed", Data: protocol.SshOutcomeHostNetworkUnconfirmedPayload{Node: address.ID, Selection: *selection}}, nil
		}
	}
	return protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: address.ID}}, nil
}

func installPreflightFailure(ctx context.Context, err error) error {
	if errors.Is(ctx.Err(), context.Canceled) {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Host installation cancelled"}
	}
	return err
}

// stageCommand makes the remote staging directory. The installer script
// needs python3, so its absence fails here, before anything is uploaded,
// and names the cause.
const stageCommand = "umask 077; command -v python3 >/dev/null || { echo '" + stagePythonRequired + "' >&2; exit 1; }; mktemp -d /tmp/sailry-bootstrap-XXXXXX"

const stagePythonRequired = "python3 is required"

// installLocal verifies and uploads the locally built Host artifacts of the
// target, then activates them with the embedded deployment scripts.
func (owner *Installations) installLocal(ctx context.Context, connection *cryptossh.Client, caller protocol.NodeID, request protocol.RequestID, root, target, turso string, paths DeploymentOptions, expected *protocol.NodeID, selection *protocol.NetworkSelection) error {
	binary, err := openArtifact(root, "sailry-host")
	if err != nil {
		return err
	}
	defer binary.file.Close()
	database, err := openArtifact(root, turso)
	if err != nil {
		return err
	}
	defer database.file.Close()
	seal, err := openArtifact(root, shipped.SealName)
	if err != nil {
		return err
	}
	defer seal.file.Close()
	manifest, err := shipped.Read(root, strings.Replace(target, "-", "/", 1))
	if err != nil {
		return unavailable("locally built Host package seal is unavailable or invalid")
	}
	entry, present := manifest.File(database.name)
	if !present || entry.Size != database.size || entry.SHA256 != database.digest {
		return unavailable("locally built Host Turso library does not match its package seal")
	}
	var installedVersion string
	if expected != nil {
		identity, err := inspectInstalled(ctx, connection, paths)
		if err != nil {
			return err
		}
		if err := validateUpdate(identity, *expected, link.ProductVersion); err != nil {
			return err
		}
		installedVersion = identity.Version
	}
	stage, err := commandSSH(ctx, connection, sshStaging, stageCommand, nil)
	if err != nil {
		return err
	}
	stage = strings.TrimSpace(stage)
	if !stagingPath.MatchString(stage) {
		return unavailable("Host installer staging directory could not be verified")
	}
	quotedStage, _ := shellQuote(stage)
	cleanupStage := true
	defer func() {
		if cleanupStage && ctx.Err() == nil {
			_, _ = commandSSH(ctx, connection, sshStaging, "rm -rf -- "+quotedStage, nil)
		}
	}()
	owner.Progress(caller, request, "installing")
	for _, input := range []*artifact{binary, database, seal} {
		destination, _ := shellQuote(stage + "/" + input.name)
		if _, err := commandSSH(ctx, connection, sshStaging, "umask 077; set -C; cat > "+destination, input.file); err != nil {
			return err
		}
	}
	service, _ := deploymentSources.ReadFile("deployment/service.sh")
	destination, _ := shellQuote(stage + "/service.sh")
	if _, err := commandSSH(ctx, connection, sshStaging, "umask 077; set -C; cat > "+destination, strings.NewReader(string(service))); err != nil {
		return err
	}
	script, err := deploymentScript(stage, binary, database, seal, paths, installedVersion, selection)
	if err != nil {
		return err
	}
	if _, err := commandSSH(ctx, connection, sshInstaller, "sh -s", strings.NewReader(script)); err != nil {
		// Only an installer whose outcome is unknown may still need the
		// stage: it holds the backup of the files it replaced.
		var fault *protocol.Fault
		cleanupStage = !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeOutcomeUnknown
		return err
	}
	return nil
}

// releaseIdentity finds the managed program files of an installed release
// Host and asks its bundled Python for the identity of the running Node.
const releaseIdentity = `install_root="$HOME/.local/lib/sailry"
if test "$(uname -s)" = Linux && test "$(id -u)" = 0; then
  if test -e /usr/local/libexec/sailry; then install_root=/usr/local/libexec/sailry
  elif ! test -L /usr/local/bin/sailry || test "$(readlink /usr/local/bin/sailry)" != "$install_root/sailry"; then
    echo 'Managed Host program files are unavailable' >&2; exit 1
  fi
fi
"$install_root/python/bin/python3.12" -I - "$install_root" "$HOME/` + identity.OfficialHostProfile + `" <<'SAILRY_IDENTITY'
`

// installRelease installs or updates the selected Host version from the
// published release feed. The bootstrap verifies install-host.sh against
// SHA256SUMS-installer; the installer verifies the target's archive against
// SHA256SUMS-<triple>. WSL installation runs the same bootstrap.
func (owner *Installations) installRelease(ctx context.Context, connection *cryptossh.Client, caller protocol.NodeID, request protocol.RequestID, expected *protocol.NodeID, selection *protocol.NetworkSelection, version string) error {
	if owner.options.isolated() {
		return invalid("isolated Host deployment requires locally built Host artifacts")
	}
	var installedVersion string
	if expected != nil {
		source, _ := deploymentSources.ReadFile("deployment/identity.py")
		body, err := commandSSH(ctx, connection, sshProbe, "sh -s", strings.NewReader(releaseIdentity+strings.ReplaceAll(string(source), "\r\n", "\n")+"\nSAILRY_IDENTITY\n"))
		if err != nil {
			return err
		}
		var identity installedIdentity
		if json.Unmarshal([]byte(body), &identity) != nil {
			return unavailable("managed Host identity could not be verified")
		}
		if err := validateUpdate(identity, *expected, version); err != nil {
			return err
		}
		installedVersion = identity.Version
	}
	if selection != nil && selection.Kind == "custom" {
		var custom protocol.NetworkSelectionCustomPayload
		body, err := json.Marshal(selection.Data)
		if err != nil || json.Unmarshal(body, &custom) != nil || len(custom.Relays) == 0 {
			return invalid("invalid relay network selection")
		}
		selection = &protocol.NetworkSelection{Kind: "custom", Data: custom}
	}
	script, err := wsl.ReleaseScript(version, expected != nil, selection)
	if err != nil {
		return err
	}
	if installedVersion != "" {
		quoted, err := shellQuote(installedVersion)
		if err != nil {
			return err
		}
		script = "export SAILRY_HOST_EXPECT_CURRENT_VERSION=" + quoted + "\n" + script
	}
	owner.Progress(caller, request, "installing")
	_, err = commandSSH(ctx, connection, sshInstaller, "sh -s", strings.NewReader(script))
	return err
}

func (owner *Installations) remotePaths(ctx context.Context, connection *cryptossh.Client) (DeploymentOptions, error) {
	if owner.options.isolated() {
		if owner.options.ProgramRoot == "" || owner.options.Profile == "" || owner.options.ServicePath == "" || owner.options.CommandPath == "" {
			return DeploymentOptions{}, invalid("Host deployment requires all isolated managed paths")
		}
		for _, path := range []string{owner.options.ProgramRoot, owner.options.Profile, owner.options.ServicePath, owner.options.CommandPath} {
			if !strings.HasPrefix(path, "/") || strings.ContainsRune(path, '\x00') {
				return DeploymentOptions{}, invalid("Host deployment paths must be absolute")
			}
		}
		return owner.options, nil
	}
	body, err := commandSSH(ctx, connection, sshProbe, "printf '%s\\n' \"$HOME\"; uname -s; id -u", nil)
	if err != nil {
		return DeploymentOptions{}, err
	}
	values := strings.Split(strings.TrimSpace(body), "\n")
	if len(values) != 3 || !strings.HasPrefix(values[0], "/") {
		return DeploymentOptions{}, unavailable("Host managed installation paths could not be verified")
	}
	home := values[0]
	paths := DeploymentOptions{ProgramRoot: home + "/.local/lib/sailry", Profile: home + "/" + identity.OfficialHostProfile, ServicePath: home + "/.config/systemd/user/sailry-host.service", CommandPath: home + "/.local/bin/sailry"}
	if values[1] == "Linux" && values[2] == "0" {
		paths.ProgramRoot, paths.ServicePath, paths.CommandPath = "/usr/local/libexec/sailry", "/etc/systemd/system/sailry-host.service", "/usr/local/bin/sailry"
	}
	if values[1] == "Darwin" {
		paths.ServicePath = home + "/Library/LaunchAgents/ai.sailry.host.plist"
	}
	return paths, nil
}

func inspectInstalled(ctx context.Context, connection *cryptossh.Client, paths DeploymentOptions) (installedIdentity, error) {
	root, err := shellQuote(paths.ProgramRoot)
	if err != nil {
		return installedIdentity{}, err
	}
	profile, _ := shellQuote(paths.Profile)
	source, _ := deploymentSources.ReadFile("deployment/identity.py")
	body, err := commandSSH(ctx, connection, sshProbe, "python3 -I - "+root+" "+profile, strings.NewReader(string(source)))
	if err != nil {
		return installedIdentity{}, err
	}
	var identity installedIdentity
	if json.Unmarshal([]byte(body), &identity) != nil {
		return identity, unavailable("managed Host identity could not be verified")
	}
	return identity, nil
}

func deploymentScript(stage string, binary, database, seal *artifact, paths DeploymentOptions, installedVersion string, selection *protocol.NetworkSelection) (string, error) {
	if !semver.IsValid("v" + link.ProductVersion) {
		return "", invalid("invalid target Host version")
	}
	values := []struct{ name, value string }{
		{"install_stage", stage}, {"install_turso", database.name},
		{"install_digest", binary.digest}, {"install_turso_digest", database.digest}, {"install_seal_digest", seal.digest},
		{"install_root", paths.ProgramRoot}, {"install_data", paths.Profile}, {"install_service_path", paths.ServicePath},
		{"install_command", paths.CommandPath}, {"install_version", link.ProductVersion},
		{"SAILRY_HOST_EXPECT_CURRENT_VERSION", installedVersion},
	}
	var script strings.Builder
	for _, value := range values {
		quoted, err := shellQuote(value.value)
		if err != nil {
			return "", err
		}
		script.WriteString("export " + value.name + "=" + quoted + "\n")
	}
	if installedVersion != "" {
		script.WriteString("export install_update=1\n")
	} else {
		script.WriteString("export install_update=0\n")
	}
	arguments := []string{}
	if selection != nil {
		switch selection.Kind {
		case "direct":
			arguments = append(arguments, "--internet")
		case "custom":
			var custom protocol.NetworkSelectionCustomPayload
			body, err := json.Marshal(selection.Data)
			if err != nil || json.Unmarshal(body, &custom) != nil || len(custom.Relays) == 0 {
				return "", invalid("invalid relay network selection")
			}
			for _, relay := range custom.Relays {
				arguments = append(arguments, "--relay", relay)
			}
		default:
			return "", invalid("invalid Host network selection")
		}
	}
	script.WriteString("set --")
	for _, argument := range arguments {
		quoted, err := shellQuote(argument)
		if err != nil {
			return "", err
		}
		script.WriteString(" " + quoted)
	}
	source, _ := deploymentSources.ReadFile("deployment/release.sh")
	script.WriteString("\n" + strings.ReplaceAll(string(source), "\r\n", "\n"))
	return script.String(), nil
}

// pairingAttempts is how many times an installation tries to pair with the
// Host it installed.
const pairingAttempts = 5

func pairInstalled(ctx context.Context, connection *cryptossh.Client, endpoint *link.Link, profile, version string) (link.EndpointAddress, error) {
	ticketPath, _ := shellQuote(profile + "/bootstrap.ticket")
	read := func() (string, error) {
		result, err := runSSH(ctx, connection, sshProbe, "cat "+ticketPath+" 2>/dev/null", nil)
		if err != nil {
			return "", err
		}
		if result.ExitCode != 0 || result.Truncated {
			return "", nil
		}
		return strings.TrimSpace(result.Stdout), nil
	}
	pair := func(ticket string) (link.EndpointAddress, error) {
		address, err := endpoint.Pair(ctx, ticket)
		if err != nil {
			return address, err
		}
		info, err := endpoint.Inspect(ctx, address)
		if err != nil || !info.Execution {
			return address, installFault(protocol.ErrorCodeOutcomeUnknown, "installed endpoint does not provide a Node", protocol.SSHPairingUnconfirmed)
		}
		if version != "" && (!sameRelease(info.SoftwareVersion, version) || info.Compatibility() != nil) {
			return address, installFault(protocol.ErrorCodeOutcomeUnknown, "installed endpoint does not provide the expected compatible Host release", protocol.SSHPairingUnconfirmed)
		}
		return address, nil
	}
	return pairBootstrap(ctx, read, pair, waitProbe)
}

// pairBootstrap pairs with a new Host through the ticket its service
// publishes. The service has only just started, so one refused or unreachable
// attempt does not end the installation: the ticket is read again, because the
// Host replaces one that expired, and pairing is tried up to pairingAttempts
// times within the 90 one-second probes. A result that may have changed trust
// is not retried.
func pairBootstrap(ctx context.Context, read func() (string, error), pair func(string) (link.EndpointAddress, error), wait func(context.Context) error) (link.EndpointAddress, error) {
	failures := 0
	for attempt := 0; attempt < 90; attempt++ {
		ticket, err := read()
		if err != nil {
			return link.EndpointAddress{}, installFault(protocol.ErrorCodeOutcomeUnknown, "Host installed but its bootstrap ticket could not be read", protocol.SSHBootstrapTicketUnavailable)
		}
		if ticket != "" {
			address, err := pair(ticket)
			if err == nil {
				return address, nil
			}
			var settled *protocol.Fault
			if errors.As(err, &settled) && settled.Code == protocol.ErrorCodeOutcomeUnknown {
				failure := *settled
				failure.SSHReason = protocol.SSHPairingUnconfirmed
				return address, &failure
			}
			if failures++; failures == pairingAttempts {
				break
			}
		}
		if err := wait(ctx); err != nil {
			return link.EndpointAddress{}, installFault(protocol.ErrorCodeOutcomeUnknown, "Host installation interrupted; inspect the Host before retrying", protocol.SSHPairingUnconfirmed)
		}
	}
	if failures > 0 {
		return link.EndpointAddress{}, installFault(protocol.ErrorCodeOutcomeUnknown, "Host installed but pairing could not be confirmed", protocol.SSHPairingUnconfirmed)
	}
	return link.EndpointAddress{}, installFault(protocol.ErrorCodeOutcomeUnknown, "Host service started but pairing is unavailable; inspect the host service", protocol.SSHBootstrapTicketUnavailable)
}

// confirmUpdate waits for the updated Host to answer with this release. The
// update replaced the Host process, so the connection cached before it is
// dropped first, and again after every probe that did not confirm: a probe
// that reached the old process must not leave its connection for the next.
func confirmUpdate(ctx context.Context, endpoint *link.Link, expected protocol.NodeID, version string) error {
	for attempt := 0; attempt < 90; attempt++ {
		endpoint.Disconnect(expected)
		peers, err := endpoint.Peers(ctx)
		if err != nil {
			return installFault(protocol.ErrorCodeOutcomeUnknown, "Host update was started but reconnection could not be confirmed", protocol.SSHUpdateUnconfirmed)
		}
		for _, address := range peers {
			if address.ID != expected {
				continue
			}
			info, err := endpoint.Inspect(ctx, address)
			if err == nil && info.Execution && sameRelease(info.SoftwareVersion, version) && info.Compatibility() == nil {
				return nil
			}
		}
		if err := waitProbe(ctx); err != nil {
			break
		}
	}
	return installFault(protocol.ErrorCodeOutcomeUnknown, "Host update was started but reconnection could not be confirmed", protocol.SSHUpdateUnconfirmed)
}

func waitProbe(ctx context.Context) error {
	timer := time.NewTimer(time.Second)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

func ConfigureNetwork(ctx context.Context, endpoint *link.Link, address link.EndpointAddress, id protocol.RequestID, selected protocol.NetworkSelection) error {
	controller := client.New(endpoint.Remote(address))
	defer controller.Close()
	read := func() (protocol.NetworkSettings, error) {
		output, err := controller.Do(ctx, protocol.Command{Kind: "read_network_settings"})
		if err != nil {
			return protocol.NetworkSettings{}, err
		}
		settings, ok := output.Data.(*protocol.NetworkSettings)
		if output.Kind != "network_settings" || !ok {
			return protocol.NetworkSettings{}, invalid("network settings response expected")
		}
		return *settings, nil
	}
	settings, err := read()
	if err != nil {
		return err
	}
	if settings.Revision > 0 {
		if !reflect.DeepEqual(settings.Selection, &selected) || !reflect.DeepEqual(settings.Effective, &selected) {
			return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "installed Host network changed"}
		}
	} else {
		request := controller.Prepare(protocol.Command{Kind: "save_network_settings", Data: protocol.CommandSaveNetworkSettingsPayload{ExpectedRevision: 0, Selection: selected}})
		request.ID = id
		output, err := controller.Settle(ctx, request)
		if err != nil {
			return err
		}
		saved, ok := output.Data.(*protocol.NetworkSettings)
		if output.Kind != "network_settings" || !ok {
			return invalid("network settings response expected")
		}
		settings = *saved
	}
	var actual link.EndpointAddress
	if json.Unmarshal([]byte(settings.Address), &actual) != nil {
		return invalid("invalid Host network address")
	}
	if actual.ID != address.ID {
		return &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "network settings belong to another Host"}
	}
	if err := endpoint.UpdatePeer(ctx, actual); err != nil {
		return err
	}
	_ = controller.Close()
	controller = client.New(endpoint.Remote(actual))
	defer controller.Close()
	verified, err := read()
	if err != nil {
		return err
	}
	if verified.Revision == 0 || !reflect.DeepEqual(verified.Selection, &selected) || !reflect.DeepEqual(verified.Effective, &selected) {
		return unknown("Host network change could not be confirmed")
	}
	return nil
}
