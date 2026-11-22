package identity

import (
	"errors"
	"regexp"
	"strings"
)

type HostRelease struct {
	repository string
	pkg        HostPackage
}

type HostPackage struct {
	name        string
	launchAgent string
}

const (
	officialHostName  = "sailry"
	officialHostAgent = "ai.sailry.host"
)

func (pkg HostPackage) Name() string        { return pkg.name }
func (pkg HostPackage) Binary() string      { return pkg.name + "-host" }
func (pkg HostPackage) Profile() string     { return "." + pkg.Binary() }
func (pkg HostPackage) LaunchAgent() string { return pkg.launchAgent }

var (
	hostRepository = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9][A-Za-z0-9_.-]*$`)
	hostName       = regexp.MustCompile(`^[a-z][a-z0-9-]*$`)
	hostLaunch     = regexp.MustCompile(`^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*){2,}$`)
)

// OfficialHost selects the latest stable published official Host independently
// of the controller build. The zero value retains the bundled Harness version.
func OfficialHost() HostRelease { return HostRelease{repository: OfficialRepository} }

// IndependentHost selects the fork's published installer and the namespace its
// package uses for the program, command, profile and services.
func IndependentHost(repository, name, launchAgent string) (HostRelease, error) {
	if !hostRepository.MatchString(repository) || strings.EqualFold(repository, OfficialRepository) {
		return HostRelease{}, errors.New("independent Host requires its own GitHub repository")
	}
	if !hostName.MatchString(name) || name == officialHostName || !hostLaunch.MatchString(launchAgent) || launchAgent == officialHostAgent {
		return HostRelease{}, errors.New("independent Host requires its own installation and service namespace")
	}
	return HostRelease{repository: repository, pkg: HostPackage{name: name, launchAgent: launchAgent}}, nil
}

func (release HostRelease) Published() bool { return release.repository != "" }

func (release HostRelease) Independent() bool { return release.pkg.name != "" }

func (release HostRelease) Package() HostPackage {
	if release.Independent() {
		return release.pkg
	}
	return HostPackage{name: officialHostName, launchAgent: officialHostAgent}
}

func (release HostRelease) Releases() string {
	if release.repository == "" {
		return OfficialReleases
	}
	return "https://api.github.com/repos/" + release.repository + "/releases"
}

func (release HostRelease) Downloads() string {
	if release.repository == "" {
		return OfficialDownloads
	}
	return "https://github.com/" + release.repository + "/releases/download"
}
