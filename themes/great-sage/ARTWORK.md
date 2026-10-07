# Great Sage artwork

Original Sun Wukong / Great Sage theme artwork generated with the built-in OpenAI
imagegen tool. The approved AI-generated Sailry theme concept was the hero's style
reference; the lower-right corner used the generated upper-left ornament as its
edit target. No external game artwork was imported. Package and artwork are
licensed under Apache-2.0, as specified in `theme.json`.

Production assets are compressed indexed PNGs with transparency. The landscape
retains its generated resolution; the hero, corner ornaments and send glyph are
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

### send.png

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

### circlet.png

Use case: stylized-concept. Asset type: transparent PNG top-edge ornament for a desktop New Conversation button in a Sun Wukong theme. Primary request: a refined thin golden circlet (Jingu) running horizontally, two ends curl upwards in small traditional cloud scroll shapes. Front view, a very wide thin shallow arc, occupying only a narrow horizontal strip at the center of a landscape canvas. Antique polished gold with finely engraved relief lines, elegant warm highlights and dark bronze grooves, tiny vermilion accents only on the curls. The center band must be slim to leave room for a button label below. No text, no button, no panel, no other objects, no background or shadow outside the metal. Genuine transparent background, crisp clean isolated edges.

The circlet is downscaled to 768 × 256 and quantized to a 256-color indexed PNG
while preserving alpha. The original canvas is retained, and the narrow top-edge
slot stretches it to its configured display height. Existing corner PNGs are
reused for user messages; no duplicate resources are included.
