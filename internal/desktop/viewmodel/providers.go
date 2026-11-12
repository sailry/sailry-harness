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
	// Features are its options beyond the effort, as a fast mode.
	Features []ModelFeature
}

// ModelFeature is a feature a model declares with its options.
type ModelFeature struct {
	ID, Name string
	Options  []FeatureOption
}

// FeatureOption is a choice of a ModelFeature.
type FeatureOption struct{ ID, Name string }

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

// Channel is a configured provider. Brand is its family (Brand.ID) and
// BrandName that family's name where no offered Brand has it. Icon is the
// catalogue identifier of its mark (icons.Brand), Brand's when empty.
// Method is the Route.ID it was saved with.
type Channel struct {
	Revision        uint64
	ID, Name, Brand string
	BrandName, Icon string
	Method          string
	API, Endpoint   string
	// APIVersion is the Azure OpenAI API version, "" for every other API.
	APIVersion                string
	Auth                      Auth
	Enabled, KeySet, SignedIn bool
	Account                   string
	Models                    []ChannelModel
	Default                   int
	Quota                     *Quota
	OAuth                     [4]string
}

// Route is one way a Method connects: its wire protocol, and the product
// and region of a hosted service. A Method's routes differ in these only,
// which the configuration form switches between.
type Route struct {
	// ID is what a channel saves as its Method; API its wire protocol
	// (Channel.API).
	ID, API string
	// Endpoint is the route's address; Custom leaves it to the user.
	Endpoint string
	Custom   bool
	// Product ("metered", "coding") and Region ("china", "global") are ""
	// where the route has none; ProductName is the product's declared name.
	Product, ProductName, Region string
	// Models are the model IDs a new channel starts with.
	Models []string
}

// Method is one way to add a channel of a Brand: an API connection or an
// account sign-in (Login, the i18n key of the account's name). Access is the
// i18n key of how it connects; Keyed asks for an API key whatever Access
// says. Icon is the catalogue identifier of its mark. Routes are its
// variants; without any the method is its only route (API, Endpoint,
// Custom).
type Method struct {
	ID, Name, Access, API, Endpoint string
	Custom, Keyed                   bool
	Login, KeyURL                   string
	OAuthFields                     bool
	Icon                            string
	Routes                          []Route
}

// Brand is a family of channels and the methods that add one.
type Brand struct {
	ID, Name string
	Methods  []Method
}
type Catalog struct {
	Updated, Scores bool
	Last            string
}
