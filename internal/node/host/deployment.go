package host

import (
	"context"
	"crypto/sha256"
	"embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
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
	binary, err := openArtifact(root, "sailry-host")
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	defer binary.file.Close()
	library, err := openArtifact(root, native)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	defer library.file.Close()
	paths, err := owner.remotePaths(ctx, connection)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	if expected != nil {
		identity, err := inspectInstalled(ctx, connection, paths)
		if err != nil {
			return protocol.SshOutcome{}, err
		}
		if err := validateUpdate(identity, *expected, link.ProductVersion); err != nil {
			return protocol.SshOutcome{}, err
		}
	}
	stage, err := commandSSH(ctx, connection, "umask 077; command -v python3 >/dev/null; mktemp -d /tmp/sailry-bootstrap-XXXXXX", nil)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	stage = strings.TrimSpace(stage)
	if !stagingPath.MatchString(stage) {
		return protocol.SshOutcome{}, unavailable("Host installer staging directory could not be verified")
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
			return protocol.SshOutcome{}, err
		}
	}
	service, _ := deploymentSources.ReadFile("deployment/service.sh")
	destination, _ := shellQuote(stage + "/service.sh")
	if _, err := commandSSH(ctx, connection, "umask 077; set -C; cat > "+destination, strings.NewReader(string(service))); err != nil {
		return protocol.SshOutcome{}, err
	}
	script, err := deploymentScript(stage, native, binary.digest, library.digest, paths, expected != nil, selection)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	cleanupStage = false
	if _, err := commandSSH(ctx, connection, "sh -s", strings.NewReader(script)); err != nil {
		return protocol.SshOutcome{}, err
	}
	cleanupStage = true
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

func pairInstalled(ctx context.Context, connection *cryptossh.Client, endpoint *link.Link, profile string) (link.EndpointAddress, error) {
	ticketPath, _ := shellQuote(profile + "/bootstrap.ticket")
	for attempt := 0; attempt < 90; attempt++ {
		result, err := runSSH(ctx, connection, "cat "+ticketPath+" 2>/dev/null", nil)
		if err != nil {
			return link.EndpointAddress{}, unknown("Host installed but its bootstrap ticket could not be read")
		}
		if result.ExitCode == 0 && !result.Truncated && strings.TrimSpace(result.Stdout) != "" {
			address, err := endpoint.Pair(ctx, strings.TrimSpace(result.Stdout))
			if err != nil {
				return address, unknown("Host installed but pairing could not be confirmed")
			}
			info, err := endpoint.Inspect(ctx, address)
			if err != nil || !info.Execution {
				return address, unknown("installed endpoint does not provide a Node")
			}
			return address, nil
		}
		if err := waitProbe(ctx); err != nil {
			return link.EndpointAddress{}, err
		}
	}
	return link.EndpointAddress{}, unknown("Host service started but pairing is unavailable; inspect the host service")
}

func confirmUpdate(ctx context.Context, endpoint *link.Link, expected protocol.NodeID) error {
	for attempt := 0; attempt < 90; attempt++ {
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
