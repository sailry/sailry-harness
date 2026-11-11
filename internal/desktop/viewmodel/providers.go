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
	// The capability tags: what the model understands, can do and generates
	// (GenerateImage, GenerateImageTool, GenerateVideo, GenerateSpeech).
	// CustomCapabilities marks tags the user chose, which the catalog no
	// longer replaces.
	Vision, Tools, Reasoning bool
	Generates                []string
	CustomCapabilities       bool
}

// The values of ChannelModel.Generates. An image comes from the images
// endpoint (GenerateImage) or from the tool the conversation model hosts
// (GenerateImageTool).
const (
	GenerateImage     = "image"
	GenerateImageTool = "image_tool"
	GenerateVideo     = "video"
	GenerateSpeech    = "speech"
)

// MediaKinds are the purposes a media model is assigned to, in the order
// the settings list them.
var MediaKinds = [4]string{"vision", "image", "video", "speech"}

// Capabilities are the MediaKinds a model's tags cover.
func Capabilities(vision bool, generates []string) (kinds []string) {
	has := func(values ...string) bool {
		for _, value := range values {
			for _, generated := range generates {
				if generated == value {
					return true
				}
			}
		}
		return false
	}
	for _, kind := range MediaKinds {
		if kind == "vision" && vision || kind == "image" && has(GenerateImage, GenerateImageTool) || kind == "video" && has(GenerateVideo) || kind == "speech" && has(GenerateSpeech) {
			kinds = append(kinds, kind)
		}
	}
	return kinds
}

// CapabilityLabels are the media kinds' names as marks and filters show
// them, by i18n key.
var CapabilityLabels = map[string]string{"vision": "provider_cap_vision", "image": "provider_cap_image", "video": "provider_cap_video", "speech": "media_speech_synthesis"}

// MediaOption is a model the media settings offer for the Kinds it covers.
// Key names it among the options.
type MediaOption struct {
	Key, Label string
	Kinds      []string
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
