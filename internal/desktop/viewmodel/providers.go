package viewmodel

type Auth int

const (
	AuthKey Auth = iota
	AuthOAuth
	AuthPlugin
	AuthNone
)

type ChannelModel struct {
	ID, Name        string
	Context, Output int
	Efforts         []string
	DefaultEffort   string
}
type Quota struct{ FiveHour, SevenDay float64 }
type Channel struct {
	Revision                  uint64
	ID, Name, Brand           string
	Method                    string
	API, Endpoint             string
	Auth                      Auth
	Enabled, KeySet, SignedIn bool
	Account                   string
	Models                    []ChannelModel
	Default                   int
	Quota                     *Quota
	OAuth                     [4]string
}
type Method struct {
	ID, Name, Access, API, Endpoint string
	Custom                          bool
	Login, KeyURL                   string
	OAuthFields                     bool
}
type Brand struct {
	ID, Name string
	Methods  []Method
}
type Catalog struct {
	Updated, Scores bool
	Last            string
}
