package link

import "github.com/sailry/sailry-harness/internal/protocol"

var ProductVersion = "0.1.5"

func CheckHost(hostProtocol *uint16, hostVersion *string) error {
	local := uint16(protocol.VERSION)
	return checkCompatibility(&local, &ProductVersion, hostProtocol, hostVersion)
}

func checkCompatibility(controllerProtocol *uint16, controllerVersion *string, hostProtocol *uint16, hostVersion *string) error {
	if controllerProtocol != nil && hostProtocol != nil && *controllerProtocol == *hostProtocol {
		return nil
	}
	updateHost := false
	message := "Controller protocol could not be verified; update the controller"
	switch {
	case controllerProtocol == nil:
	case hostProtocol == nil:
		updateHost = true
		message = "Host protocol could not be verified; update Host"
	case *controllerProtocol > *hostProtocol:
		updateHost = true
		message = "Host protocol is older; update Host"
	default:
		message = "Host protocol is newer; update the controller"
	}
	return &protocol.Fault{
		Code: protocol.ErrorCodeConflict, Message: message,
		Details: &protocol.FaultDetails{
			Kind: "version_mismatch",
			Data: protocol.FaultDetailsVersionMismatchPayload{
				Controller: controllerVersion, Host: hostVersion,
				ControllerProtocol: controllerProtocol, HostProtocol: hostProtocol,
				UpdateHost: updateHost, Unverified: controllerProtocol == nil || hostProtocol == nil,
			},
		},
	}
}
