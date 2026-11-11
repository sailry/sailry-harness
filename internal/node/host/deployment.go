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
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/wsl"
	"github.com/sailry/sailry-harness/internal/protocol"
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
	if _, err = io.Copy(digest, file); err == nil {
		_, err = file.Seek(0, io.SeekStart)
	}
	if err != nil {
		_ = file.Close()
		return nil, unavailable("locally built Host artifact could not be verified")
	}
	return &artifact{file: file, digest: hex.EncodeToString(digest.Sum(nil)), name: name}, nil
}

func targetPlatform(platform string) (string, string, error) {
	switch strings.Join(strings.Fields(platform), "/") {
	case "Linux/x86_64":
		return "linux-amd64", "libsailry_iroh.so", nil
	case "Linux/aarch64", "Linux/arm64":
		return "linux-arm64", "libsailry_iroh.so", nil
	case "Darwin/arm64":
		return "darwin-arm64", "libsailry_iroh.dylib", nil
	case "Darwin/x86_64":
		return "darwin-amd64", "libsailry_iroh.dylib", nil
	}
	return "", "", invalid("Host installation supports Linux and macOS on amd64 or arm64")
}

func validateUpdate(identity installedIdentity, node protocol.NodeID, target string) error {
	if identity.Node != node {
		return &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "SSH endpoint belongs to another Host"}
	}
	current := strings.TrimPrefix(identity.Version, "Sailry Host ")
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

func (owner *Installations) Execute(ctx context.Context, connection *cryptossh.Client, endpoint *link.Link, caller protocol.NodeID, request protocol.RequestID, expected *protocol.NodeID, selection *protocol.NetworkSelection) (protocol.SshOutcome, error) {
	ctx, cancel := context.WithTimeout(ctx, 600*time.Second)
	defer cancel()
	owner.Progress(caller, request, "detecting")
	platform, err := commandSSH(ctx, connection, "uname -s; uname -m", nil)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	target, native, err := targetPlatform(platform)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	root, err := artifactDirectory(target)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	paths, err := owner.remotePaths(ctx, connection)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	// A build that carries locally built artifacts for the target uploads
	// them; every other build installs its own version from the release feed.
	if _, missing := os.Lstat(filepath.Join(root, "sailry-host")); errors.Is(missing, fs.ErrNotExist) {
		err = owner.installRelease(ctx, connection, caller, request, expected, selection)
	} else {
		err = owner.installLocal(ctx, connection, caller, request, root, native, paths, expected, selection)
	}
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	owner.Progress(caller, request, "pairing")
	if expected != nil {
		if err := confirmUpdate(ctx, endpoint, *expected); err != nil {
			return protocol.SshOutcome{}, err
		}
		return protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: *expected}}, nil
	}
	address, err := pairInstalled(ctx, connection, endpoint, paths.Profile)
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

// installLocal verifies and uploads the locally built Host artifacts of the
// target, then activates them with the embedded deployment scripts.
func (owner *Installations) installLocal(ctx context.Context, connection *cryptossh.Client, caller protocol.NodeID, request protocol.RequestID, root, native string, paths DeploymentOptions, expected *protocol.NodeID, selection *protocol.NetworkSelection) error {
	binary, err := openArtifact(root, "sailry-host")
	if err != nil {
		return err
	}
	defer binary.file.Close()
	library, err := openArtifact(root, native)
	if err != nil {
		return err
	}
	defer library.file.Close()
	if expected != nil {
		identity, err := inspectInstalled(ctx, connection, paths)
		if err != nil {
			return err
		}
		if err := validateUpdate(identity, *expected, link.ProductVersion); err != nil {
			return err
		}
	}
	stage, err := commandSSH(ctx, connection, "umask 077; command -v python3 >/dev/null; mktemp -d /tmp/sailry-bootstrap-XXXXXX", nil)
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
			_, _ = commandSSH(ctx, connection, "rm -rf -- "+quotedStage, nil)
		}
	}()
	owner.Progress(caller, request, "installing")
	for _, input := range []*artifact{binary, library} {
		destination, _ := shellQuote(stage + "/" + input.name)
		if _, err := commandSSH(ctx, connection, "umask 077; set -C; cat > "+destination, input.file); err != nil {
			return err
		}
	}
	service, _ := deploymentSources.ReadFile("deployment/service.sh")
	destination, _ := shellQuote(stage + "/service.sh")
	if _, err := commandSSH(ctx, connection, "umask 077; set -C; cat > "+destination, strings.NewReader(string(service))); err != nil {
		return err
	}
	script, err := deploymentScript(stage, native, binary.digest, library.digest, paths, expected != nil, selection)
	if err != nil {
		return err
	}
	cleanupStage = false
	if _, err := commandSSH(ctx, connection, "sh -s", strings.NewReader(script)); err != nil {
		return err
	}
	cleanupStage = true
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
"$install_root/python/bin/python3.12" -I - "$install_root" "$HOME/.sailry-host" <<'SAILRY_IDENTITY'
`

// installRelease installs or updates this application version from the
// published release feed. The bootstrap verifies install-host.sh against
// SHA256SUMS-installer; the installer verifies the target's archive against
// SHA256SUMS-<triple>. WSL installation runs the same bootstrap.
func (owner *Installations) installRelease(ctx context.Context, connection *cryptossh.Client, caller protocol.NodeID, request protocol.RequestID, expected *protocol.NodeID, selection *protocol.NetworkSelection) error {
	if owner.options != (DeploymentOptions{}) {
		return invalid("isolated Host deployment requires locally built Host artifacts")
	}
	if expected != nil {
		source, _ := deploymentSources.ReadFile("deployment/identity.py")
		body, err := commandSSH(ctx, connection, "sh -s", strings.NewReader(releaseIdentity+strings.ReplaceAll(string(source), "\r\n", "\n")+"\nSAILRY_IDENTITY\n"))
		if err != nil {
			return err
		}
		var identity installedIdentity
		if json.Unmarshal([]byte(body), &identity) != nil {
			return unavailable("managed Host identity could not be verified")
		}
		if err := validateUpdate(identity, *expected, link.ProductVersion); err != nil {
			return err
		}
	}
	if selection != nil && selection.Kind == "custom" {
		var custom protocol.NetworkSelectionCustomPayload
		body, err := json.Marshal(selection.Data)
		if err != nil || json.Unmarshal(body, &custom) != nil || len(custom.Relays) == 0 {
			return invalid("invalid relay network selection")
		}
		selection = &protocol.NetworkSelection{Kind: "custom", Data: custom}
	}
	script, err := wsl.ReleaseScript(link.ProductVersion, expected != nil, selection)
	if err != nil {
		return err
	}
	owner.Progress(caller, request, "installing")
	_, err = commandSSH(ctx, connection, "sh -s", strings.NewReader(script))
	return err
}

func (owner *Installations) remotePaths(ctx context.Context, connection *cryptossh.Client) (DeploymentOptions, error) {
	configured := owner.options.ProgramRoot != "" || owner.options.Profile != "" || owner.options.ServicePath != "" || owner.options.CommandPath != ""
	if configured {
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
	body, err := commandSSH(ctx, connection, "printf '%s\\n' \"$HOME\"; uname -s; id -u", nil)
	if err != nil {
		return DeploymentOptions{}, err
	}
	values := strings.Split(strings.TrimSpace(body), "\n")
	if len(values) != 3 || !strings.HasPrefix(values[0], "/") {
		return DeploymentOptions{}, unavailable("Host managed installation paths could not be verified")
	}
	home := values[0]
	paths := DeploymentOptions{ProgramRoot: home + "/.local/lib/sailry", Profile: home + "/.sailry-host", ServicePath: home + "/.config/systemd/user/sailry-host.service", CommandPath: home + "/.local/bin/sailry"}
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
	body, err := commandSSH(ctx, connection, "python3 -I - "+root+" "+profile, strings.NewReader(string(source)))
	if err != nil {
		return installedIdentity{}, err
	}
	var identity installedIdentity
	if json.Unmarshal([]byte(body), &identity) != nil {
		return identity, unavailable("managed Host identity could not be verified")
	}
	return identity, nil
}

func deploymentScript(stage, native, digest, nativeDigest string, paths DeploymentOptions, update bool, selection *protocol.NetworkSelection) (string, error) {
	if !semver.IsValid("v" + link.ProductVersion) {
		return "", invalid("invalid target Host version")
	}
	values := []struct{ name, value string }{
		{"install_stage", stage}, {"install_native", native}, {"install_digest", digest}, {"install_native_digest", nativeDigest},
		{"install_root", paths.ProgramRoot}, {"install_data", paths.Profile}, {"install_service_path", paths.ServicePath},
		{"install_command", paths.CommandPath}, {"install_version", link.ProductVersion},
	}
	var script strings.Builder
	for _, value := range values {
		quoted, err := shellQuote(value.value)
		if err != nil {
			return "", err
		}
		script.WriteString("export " + value.name + "=" + quoted + "\n")
	}
	if update {
		script.WriteString("export install_update=1\n")
	} else {
		script.WriteString("export install_update=0\n")
	}
	arguments := []string{}
	if selection != nil {
		switch selection.Kind {
		case "iroh":
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

func pairInstalled(ctx context.Context, connection *cryptossh.Client, endpoint *link.Link, profile string) (link.EndpointAddress, error) {
	ticketPath, _ := shellQuote(profile + "/bootstrap.ticket")
	read := func() (string, error) {
		result, err := runSSH(ctx, connection, "cat "+ticketPath+" 2>/dev/null", nil)
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
			return address, unknown("installed endpoint does not provide a Node")
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
			return link.EndpointAddress{}, unknown("Host installed but its bootstrap ticket could not be read")
		}
		if ticket != "" {
			address, err := pair(ticket)
			if err == nil {
				return address, nil
			}
			var settled *protocol.Fault
			if errors.As(err, &settled) && settled.Code == protocol.ErrorCodeOutcomeUnknown {
				return address, err
			}
			if failures++; failures == pairingAttempts {
				break
			}
		}
		if err := wait(ctx); err != nil {
			return link.EndpointAddress{}, err
		}
	}
	if failures > 0 {
		return link.EndpointAddress{}, unknown("Host installed but pairing could not be confirmed")
	}
	return link.EndpointAddress{}, unknown("Host service started but pairing is unavailable; inspect the host service")
}

// confirmUpdate waits for the updated Host to answer with this release. The
// update replaced the Host process, so the connection cached before it is
// dropped first, and again after every probe that did not confirm: a probe
// that reached the old process must not leave its connection for the next.
func confirmUpdate(ctx context.Context, endpoint *link.Link, expected protocol.NodeID) error {
	for attempt := 0; attempt < 90; attempt++ {
		endpoint.Disconnect(expected)
		peers, err := endpoint.Peers(ctx)
		if err != nil {
			return err
		}
		for _, address := range peers {
			if address.ID != expected {
				continue
			}
			info, err := endpoint.Inspect(ctx, address)
			if err == nil && info.Execution && sameRelease(info.SoftwareVersion, link.ProductVersion) {
				return nil
			}
		}
		if err := waitProbe(ctx); err != nil {
			return err
		}
	}
	return unknown("Host update was started but reconnection could not be confirmed")
}

func waitProbe(ctx context.Context) error {
	timer := time.NewTimer(time.Second)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return unknown("Host installation interrupted; inspect the Host before retrying")
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
		if reflect.DeepEqual(settings.Selection, &selected) && reflect.DeepEqual(settings.Effective, &selected) {
			return nil
		}
		return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "installed Host network changed"}
	}
	request := controller.Prepare(protocol.Command{Kind: "save_network_settings", Data: protocol.CommandSaveNetworkSettingsPayload{ExpectedRevision: 0, Selection: selected}})
	request.ID = id
	output, err := controller.Execute(ctx, request)
	if err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeOutcomeUnknown {
			_, _ = controller.Outcome(ctx, request)
		}
		return err
	}
	saved, ok := output.Data.(*protocol.NetworkSettings)
	if output.Kind != "network_settings" || !ok {
		return invalid("network settings response expected")
	}
	var actual link.EndpointAddress
	if json.Unmarshal([]byte(saved.Address), &actual) != nil {
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
