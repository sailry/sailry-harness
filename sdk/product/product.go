// Package product defines the identity and release inputs of a desktop product.
package product

import "github.com/sailry/sailry-harness/internal/identity"

type Desktop = identity.Desktop
type Release = identity.DesktopRelease
type Package = identity.DesktopPackage
type HostRelease = identity.HostRelease

func CodeHost() HostRelease { return identity.CodeHost() }
