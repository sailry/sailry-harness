# Ghostty terminal engine

Desktop's terminal uses the terminal plugin of the pinned MyGo version. Its
manifest (`plugins/terminal/mygo-plugin.json` in the MyGo module) pins the
`libghostty-vt` shared library built from
https://github.com/ghostty-org/ghostty/tree/befcdfd2c3a1cb24d9ec886e93c95b2b5daa7028
with a SHA-256 per target, and `mygo build` downloads it into the Desktop
package. Sailry does not modify the library.

Ghostty is licensed under MIT; the notice is retained in
[ghostty-MIT.txt](ghostty-MIT.txt). No Ghostty shell integration script is
imported.
