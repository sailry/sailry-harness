package link

import (
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/mod/semver"
)

var ProductVersion = "0.1.6"

func CheckHost(hostProtocol *uint16, hostVersion *string) error {
	local := uint16(protocol.VERSION)
	return checkCompatibility(&local, &ProductVersion, hostProtocol, hostVersion)
}

// checkCompatibility decides whether a controller and a Host can exchange
// execution calls. The protocol number decides first. Sailry's contracts stay
// at v1 and change in place until the first release, so two builds with the
// same number can still disagree; until then the releases must also be the
// same, compared by semantic-version precedence without build metadata. The
// older side is the one to update, and a side whose release cannot be read is
// treated as the older one.
func checkCompatibility(controllerProtocol *uint16, controllerVersion *string, hostProtocol *uint16, hostVersion *string) error {
	updateHost, unverified := false, controllerProtocol == nil || hostProtocol == nil
	message := "Controller protocol could not be verified; update the controller"
	switch {
	case controllerProtocol == nil:
	case hostProtocol == nil:
		updateHost = true
		message = "Host protocol could not be verified; update Host"
	case *controllerProtocol > *hostProtocol:
		updateHost = true
		message = "Host protocol is older; update Host"
	case *controllerProtocol < *hostProtocol:
		message = "Host protocol is newer; update the controller"
	default:
		controller, host := release(controllerVersion), release(hostVersion)
		order := semver.Compare(controller, host)
		switch {
		case controller == "":
			unverified = true
			message = "Controller version could not be verified; update the controller"
		case host == "":
			updateHost, unverified = true, true
			message = "Host version could not be verified; update Host"
		case order > 0:
			updateHost = true
			message = "Host version is older; update Host"
		case order < 0:
			message = "Host version is newer; update the controller"
		default:
			return nil
		}
	}
	return &protocol.Fault{
		Code: protocol.ErrorCodeConflict, Message: message,
		Details: &protocol.FaultDetails{
			Kind: "version_mismatch",
			Data: protocol.FaultDetailsVersionMismatchPayload{
				Controller: controllerVersion, Host: hostVersion,
				ControllerProtocol: controllerProtocol, HostProtocol: hostProtocol,
				UpdateHost: updateHost, Unverified: unverified,
			},
		},
	}
}

// release is a product version in the form semver compares, or empty when it
// is missing or not a version.
func release(version *string) string {
	if version == nil || !semver.IsValid("v"+*version) {
		return ""
	}
	return "v" + *version
}
