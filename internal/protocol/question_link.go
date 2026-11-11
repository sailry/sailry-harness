package protocol

import (
	"net/netip"
	"net/url"
	"strings"
	"unicode"
)

// QuestionLinkAllowed reports whether an MCP question may ask the user to open
// the address: HTTPS, or HTTP to this machine, with a host and no user
// information. The Node refuses any other link and a controller opens none.
func QuestionLinkAllowed(value string) bool {
	if strings.ContainsFunc(value, unicode.IsControl) {
		return false
	}
	address, err := url.Parse(value)
	if err != nil || address.User != nil || address.Hostname() == "" {
		return false
	}
	switch address.Scheme {
	case "https":
		return true
	case "http":
		host := address.Hostname()
		ip, err := netip.ParseAddr(host)
		return strings.EqualFold(host, "localhost") || err == nil && ip.IsLoopback()
	}
	return false
}
