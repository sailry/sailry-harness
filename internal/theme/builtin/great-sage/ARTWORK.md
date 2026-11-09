# Great Sage artwork

Original Sun Wukong / Great Sage theme artwork generated with the built-in OpenAI
imagegen tool. The approved AI-generated Sailry theme concept was the hero's style
reference; the lower-right corner used the generated upper-left ornament as its
edit target. No external game artwork was imported. Package and artwork are
licensed under Apache-2.0, as specified in `theme.json`.

Production assets are compressed indexed PNGs with transparency. The landscape
retains its generated resolution; the hero and corner ornaments are
sized for their display roles. The package shares artwork across variants and
sets background opacity separately for light and dark modes.

## Generation prompts

### hero.png

Use case: illustration-story. Project asset: transparent welcome hero for the Great Sage desktop theme. The reference image is ONLY the approved visual style reference, not an edit target. Generate ONLY the ORIGINAL Sun Wukong bust illustration from the reference, no interface, no text, no Sailry wordmark. Wide landscape canvas about 3:2, bust centered with all crown and swept red phoenix plumes contained in canvas, black and antique gold engraved armor, monkey face, small crimson scarf, gold filigree crown. Painterly mythological Chinese illustration, poised fierce expression, restrained ink and gold look. Bust should fade naturally at lower edge with subtle scattered gold ink, genuine transparent alpha around the figure, no black rectangle, no checkerboard drawn in image. It must remain legible at 220px display height against black or ivory. Original character design, not a game character.

### background.png

Use case: style-transfer. Project asset: replacement background decoration PNG for Sailry's Great Sage theme. The existing landscape is the edit target; the approved UI concept is the style reference only. Output only background artwork, never interface, lettering, portrait or controls. High-resolution 4:3 landscape, genuine transparent alpha with no paper or black rectangle. Match the reference's antique gold and vermilion ink atmosphere: hollow gold-outline auspicious clouds at the top-right and lower corners, flowing gold ribbons and sparse dry-brush dust, low charcoal mountains with gold edges along the bottom. No massive right-hand mountain or cream-filled cloud masses. Broad muted vermilion and burgundy ink washes across the upper third, tapering toward the sides. Keep the central lower content area calm and mostly transparent without leaving most of the canvas empty. Insets should survive a centered cover crop. No text, logo, portrait, GUI, watermark or painted checkerboard. Shared transparent art works at lower opacity over ivory.

### cloud-top-left.png

Use case: illustration-story. Asset type: reusable transparent auspicious-cloud corner ornament for a black, gold and red Chinese Great Sage desktop theme. Generate ONE square PNG with genuine transparent background. A delicate antique-gold engraved Chinese 祥云 cloud curl ornament occupies the upper-left corner and extends right and down in a loose L shape. Very sparse flowing gold outlines, a tiny muted crimson ink accent, generous fully transparent negative space over the remaining lower-right 75 percent. All strokes inside canvas with comfortable margin. Designed to be shown at 48 pixels, simplified clear cloud curls with moderate stroke width. No text, no monkey, no scene, no black rectangle, no checkerboard painted into image, no frame, no gold-filled panels. Elegant restrained Chinese gold-ink ornament.

### cloud-bottom-right.png

Use case: precise-object-edit. Project asset: transparent lower-right cloud corner decoration. Input image is the edit target. Rotate the entire existing gold and red upper-left L-shaped cloud ornament by exactly 180 degrees to form a lower-right L-shaped ornament. Preserve its gold engraved cloud artwork, red accents, relative proportions and genuine transparent background. Output only the rotated transparent artwork, no black fill, no text, no UI, no new objects.

### Retired send glyph

The raster glyph was removed in favor of Kit vector glyphs that follow the
composer action foreground in each variant. Its original generation prompt is
retained for provenance.

Use case: ui-mockup. Project asset: ONE small transparent PNG send-button glyph for an original Great Sage Sun Wukong theme. Centered square canvas. A clear bold UPWARD ARROW with a straight shaft subtly resembling a red-and-black Ruyi Jingu Bang staff, two tiny ornamental bands. The arrowhead must remain unmistakably an upward send arrow. Flat warm ink black silhouette with a tiny vermilion accent, NOT gold (it will appear on a gold circular button). Very simple graphic designed for legibility at 16px, thick strokes, minimal detail, occupies central 70% of canvas. Genuine transparent background. No button circle, no background, no glow, no shadow, no text, no other objects, no watermark.


### Card and heading icons

Use case: stylized-concept. Generate one small square transparent raster icon per asset for the Great Sage theme. Simple elegant bold silhouettes, antique gold and warm black thick ink outlines, restrained vermilion accents, clear at 24 logical pixels, central 75 percent, very few details, original flat engraved brush art. No photorealism, badge, scene, text or watermark.

- `explore.png`: one golden auspicious cloud with three curled lobes and a flowing red tail, suggesting exploration and travel
- `build.png`: one red-and-gold Ruyi Jingu Bang staff diagonally ascending, gold bands at both ends and a dark vermilion shaft
- `review.png`: one bold gold almond eye with a vermilion pupil and two controlled flame strokes, suggesting inspection
- `plan.png`: one ivory and antique-gold rolled scroll with short black ink marks rather than letters and a small vermilion ribbon

The sidebar reuses these original illustrations at 16 pixels. Production icons are compressed to 96 × 96 pixels.

### grain.png

Use case: stylized-concept. Subtle seamless tileable transparent square PNG texture for small dark-gold-red Chinese mythology cards. Only sparse antique bronze and muted gold dry-brush fibers, fine dust and tiny ink speckles, dispersed with matching edges. Genuine transparent alpha across most pixels, no paper or black rectangle. Delicate irregular grain, low contrast at reduced opacity over brown or ivory panels. No clouds, mountains, scene, shapes, text, icons, frames, broad washes or painted checkerboard.

### Button edge gilding

The rejected inset circlet frame was replaced with broken gold leaf at the actual
button edges. No separate capsule outline is included. The `button.new_session`
slot uses full-width fill at 32 logical pixels and 65% opacity, matching the Kit
button's bounds and corner clip. The texture has no geometric frame to distort,
and its transparent center leaves the live caption untouched. The 768 × 258
indexed PNG retains generated alpha; downsizing and palette compression are the
only postprocessing. The frame resource was removed from the package.

The existing `grain.png` is reused as a content-panel texture as well as on cards.
It tiles at its native 256-pixel size without generating another image. Primary
buttons use solid warm-black fills. Send and Stop use separate high-contrast
solid fills with Kit vector glyphs.

Generated with the built-in OpenAI imagegen tool using this prompt:

Use case: stylized-concept. Asset type: transparent gold-leaf EDGE TEXTURE overlay for a Sun Wukong burgundy lacquer UI button. Generate original gilding texture only, NOT a frame or object. Wide 3:1 canvas, genuine transparent alpha. Fine irregular antique gold leaf fragments and tiny burnished gold engraved streaks MUST TOUCH the actual canvas boundaries: a refined patch along the top-left edge, thinning out along the upper edge; a smaller balanced patch along the bottom-right edge, thinning along the bottom edge. The gold looks fused into worn red lacquer, but DO NOT paint any red or button surface: only the gold pixels are present. Edge zones are at most the outermost 12 percent of canvas height, and the two corner patches cover at most 18 percent of width. Keep the entire central 75 percent completely transparent for live text. Gold leaf has convincing bright micro-highlights, etched grooves, aged bronze shadows and sparse tiny flakes. Flat irregular gilded edge detail with understated luxuriant material finish, no glow. There must be NO enclosed loop, NO complete outline, NO capsule, NO oval, NO rounded rectangle, NO inner rim, NO clouds, NO circlet, NO central flourish, NO text, NO label, NO button background, NO border offset from image edges, NO transparent padding outside the outermost gold. Texture is intended to stretch across a 223x32px button, with corners clipped by the real button's radius, so avoid recognizable shapes that stretching would distort. Only the upper-left and lower-right EDGE gold textures, on transparent background.

### Send glyph refinement

Use case: precise-object-edit. Edit target: supplied small transparent upward send-arrow glyph. Change only the ink-black shaft and arrowhead to bright warm ivory-gold, preserving the same bold simple UP arrow shape, two little red staff bands, centered proportions and genuine transparent alpha. It will appear at 16px on a dark burgundy button. The shaft and arrowhead must be highly legible, thick and flat. No background, glow, shadows, new ornaments or circle.
