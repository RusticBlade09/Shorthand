# Turns the logo art (glowing S on black) into a square, transparent 1024px icon source.
# The black is removed: alpha comes from each pixel's brightest channel, with the
# faint glow cut off and the curve lifted so the S stays solid instead of washing out on light backgrounds.
# Run: python tools/make-icon.py   then   cd src-tauri && cargo tauri icon icons/source.png
from pathlib import Path
from PIL import Image

LO = 28  # brightness at or below this becomes fully transparent (background + weakest haze)

root = Path(__file__).resolve().parent.parent
img = Image.open(root / "assets" / "icon-source.jpg").convert("RGB")

# Crop to a square around everything visibly brighter than the background, plus a little margin.
box = img.convert("L").point(lambda v: 255 if v > 40 else 0).getbbox()
cx, cy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
side = max(box[2] - box[0], box[3] - box[1]) * 1.04
img = img.crop((round(cx - side / 2), round(cy - side / 2), round(cx + side / 2), round(cy + side / 2)))
img = img.resize((1024, 1024), Image.LANCZOS)


def unblack(r, g, b):
    m = max(r, g, b)
    if m <= LO:
        return (0, 0, 0, 0)
    a = ((m - LO) / (255 - LO)) ** 0.55  # lift: mid-bright pixels (the S body) turn nearly opaque
    # The art is colour * brightness over black, so dividing by the brightness restores the colour.
    return (min(255, r * 255 // m), min(255, g * 255 // m), min(255, b * 255 // m), round(a * 255))


out = Image.new("RGBA", img.size)
out.putdata([unblack(*p) for p in img.get_flattened_data()])
out.save(root / "src-tauri" / "icons" / "source.png")
print("crop box", box, "-> icons/source.png")
