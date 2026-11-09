# Gilded Ink Dragon artwork

Original mountain-and-dragon composition generated with the built-in OpenAI
imagegen tool. The approved AI-generated Sailry preview is a style and composition
reference. The dark wallpaper uses the light wallpaper as its edit target.
No external game artwork, character, interface, or logo is included. Package and
artwork are licensed under Apache-2.0, as specified in `theme.json`.

Production images are compressed indexed PNGs with genuine alpha transparency.
The wallpapers retain their generated resolution. Card and sidebar symbols share
96-pixel resources; the gold foil texture repeats at 128 logical pixels and also
supplies card grain. Dedicated ink-and-foil corner ornaments refine the composer
and message surfaces. The mountain-and-foil overlay decorates the real Kit button,
with an open center for its label. The native send arrow and particle wordmark
use the package's semantic colors.

## Generation prompts

### Card and heading icons

Each icon uses this base prompt followed by its individual subject:

Use case: stylized-concept
Asset type: one original transparent PNG app theme icon. Reference image: style reference only, not an edit target.
Modern monochrome ink and restrained metallic gold, sculptural silver highlights, crisp silhouette that remains recognizable at 24 pixels. Center the single symbol inside a square canvas with 12% empty margin, genuine alpha transparency. No text, no background, no UI, no watermark, no red seal, no extra symbols. Use broad shapes, avoid tiny illegible details. A coherent premium set for Gilded Ink Dragon, matching the reference's black/white/gold ink dragon wallpaper.

- `explore.png`: a small cluster of three dramatic angular ink mountain peaks, tallest in center-right, silver mist at the base, one gold ridgeline. A strong compact mountain symbol for Explore.
- `build.png`: a decisive sweeping black ink crescent wave with a clean silver outer edge and a single gold arc through its center. Energetic forward motion, one brushstroke symbol for Build.
- `review.png`: a modern stylized watchful dragon eye: a bold black and silver almond-shaped brushstroke around a circular gold iris, tiny black pupil. Clear eye silhouette for Review.
- `plan.png`: four beveled square tiles arranged as a compact two-by-two diamond-shaped grid in slight isometric view, three silver and black tiles and one gold tile, disconnected by clean transparent gaps. Geometric planning symbol.

### grain.png

Use case: stylized-concept
Asset type: seamless repeating transparent PNG surface texture for modern monochrome ink desktop cards and composer corner accents.
A very sparse scattering of delicate antique metallic gold foil flakes and tiny silver-gray ink specks. Most pixels completely transparent. Neutral fine irregular grain, only a few larger 1-to-3-pixel flakes, no obvious subject, no clouds, no paper rectangle, no border, no text, no symbols. Seamless tile with uniform density across edges, actual alpha transparency. High-end subtle modern foil texture, not grungy. Small square composition.

### background.png

Use case: background-extraction
Asset type: production transparent PNG wallpaper for a desktop theme named Gilded Ink Dragon.
Input image: reference for the approved artwork composition and style. Extract/recreate ONLY the background illustration, remove the entire app interface, window chrome, sidebar, cards, text, icons, logo, seals and button graphics.
Primary request: modern black-and-white Chinese ink landscape with a majestic original black dragon coiled around the tall mountain summit in the upper-right quarter, sculptural silver mane and horns, tiny molten-gold eyes, subtle gold foil highlights. The mountains MUST remain clearly visible beneath and around the dragon. Keep the approved powerful diagonal dry-brush black sweep on the right, black rocky mountains on the lower-left and lower-right edges, silver mist, scattered narrow gold foil streaks.
Composition: wide landscape, large detailed high-resolution image ideally 3072 x 1920. Keep all important dragon head and mountain peaks inside a safe margin (at least 8% from top and right). The dragon and right summit occupy the top/right perimeter. The central 55% is mostly genuinely transparent negative space for working UI. Sparse translucent silver ink washes may cross the left perimeter; the entire center and upper-left remain quiet. No opaque white rectangle or paper backdrop. Actual alpha transparency behind the ink, rocks and mist.
Style: modern premium game concept art meets expressive large splash ink, sharp silver detail in black dragon, energetic dry-brush strokes, foil gold specks. Neutral cool black, silver-white, restrained antique gold. Atmospheric, powerful, clean; not a traditional scroll painting. Preserve the exact reference's mountain-and-dragon direction and peripheral composition.
Constraints: no UI, no text, no typography, no human, no decorative seals, no red accents, no border frame, no checkerboard painted into the image. The wallpaper should overlay both light and dark window backgrounds. Keep white/silver details within black mountains and the dragon.

### background-dark.png

Use case: lighting-weather
Asset type: dark-mode transparent PNG variant of the production Gilded Ink Dragon desktop wallpaper.
Input: edit target is the generated transparent mountain-and-dragon wallpaper.
Change ONLY the illumination and ink mist for dark mode. Preserve the same dragon, upper-right mountains, exact composition, canvas proportions, diagonal brush sweep, gold foil accents and lower corner mountains. The dragon MUST remain atop the visible mountains. Keep genuine alpha transparency and the large empty center; do not fill it with a black rectangle or any backdrop.
Make the dragon's silver mane, horns, muzzle and scale ridges and the mountains' silver ridgelines more visible on a charcoal #171A1D window background. Transform pale opaque white mist into subtle translucent smoky silver mist so the image feels atmospheric on dark mode instead of a white pasted-on picture. Retain expressive black dry-brush masses, strong gold flecks and neutral cool silver highlights. Restrained, premium modern game concept art meets large splash ink. No new objects, no UI, no lettering, no seals, no colored aura or bright yellow/red fringes. Keep clean natural feathered alpha edges.
Return a detailed high-resolution PNG, ideally 2560 by 1600, without cropping any existing artwork.


### button-ink.png

Use case: stylized-concept
Asset type: transparent PNG decorative OVERLAY for a real New Conversation button in Gilded Ink Dragon, matching the button in the reference UI.
Reference image: style and composition reference only.
Generate ONLY the decoration, not a whole opaque button. Very wide horizontal landscape composition. Leftmost 18%: small powerful black ink mountain silhouettes along the lower edge, silver rock ridgelines and a restrained gold seam. Rightmost 12%: a sweeping diagonal black-and-silver dry-brush splash with gold foil flecks reaching the edge. A very thin fractured antique gold trim along the top and bottom edges may join the ends. The central 60% MUST be completely alpha-transparent and empty, except an extremely thin top/bottom trim. It will sit above live silver-white button text; do not cover the label or icon.
Modern premium black/white ink, small gold highlights, expressive and elegant. Silver highlights keep the decoration visible over an ink-black button. No text, no seals, no red, no circle, no background, no filled rectangle, no painted checkerboard, no extra objects. Keep mountains compact and low, within the lower 50% of the canvas. Genuine alpha transparency. Intended display is approximately 240 by 32 logical pixels; keep silhouettes simple and visible at this height.

### corner-top-left.png

Use case: stylized-concept
Asset type: ONE transparent PNG top-left composer and message corner ornament for Gilded Ink Dragon.
Reference: modern black/white ink with gold foil, style only.
Square canvas. A crisp but restrained L-shaped splash follows the TOP and LEFT edges: broken fine antique-gold foil line, a few brighter gold flakes, one small silver dry-brush stroke and tiny ink splashes. Artwork occupies only the top-left 25%, extending a little right and down. The lower-right 75% remains fully transparent for text. Modern, abstract, sharp premium details, visible at 48 pixels, not a traditional ornate scroll. No clouds, no characters, no text, no seal, no frame around all four sides, no background, no painted checkerboard. Genuine alpha transparency.

### corner-bottom-right.png

Use case: stylized-concept
Asset type: ONE transparent PNG bottom-right composer and message corner ornament for Gilded Ink Dragon.
Reference: modern black/white ink with gold foil, style only.
Square canvas. A crisp restrained L-shaped splash follows the BOTTOM and RIGHT edges: broken fine antique-gold foil line, a few brighter gold flakes, one small silver dry-brush stroke and tiny ink splashes. Artwork occupies only the bottom-right 25%, extending a little left and up. The upper-left 75% remains fully transparent for controls. Modern abstract sharp premium details, visible at 48 pixels, matching a top-left counterpart. No clouds, no characters, no text, no seal, no frame around all four sides, no background, no painted checkerboard. Genuine alpha transparency.
