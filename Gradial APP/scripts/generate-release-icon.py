from pathlib import Path

from PIL import Image, ImageDraw


CANVAS = 1024
BACKGROUND = "#171923"
ACCENT = "#8797ff"
FOREGROUND = "#f1f3ff"


def main() -> None:
    repository = Path(__file__).resolve().parents[2]
    resources = repository / "Gradial APP" / "crates" / "gradial-app" / "resources"
    assets = repository / "assets"
    resources.mkdir(parents=True, exist_ok=True)
    assets.mkdir(parents=True, exist_ok=True)

    image = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((24, 24, 1000, 1000), radius=220, fill=BACKGROUND)

    stroke = 106
    bounds = (218, 218, 806, 806)
    draw.arc(bounds, start=30, end=326, fill=ACCENT, width=stroke)
    draw.line((512, 512, 792, 512, 792, 742), fill=ACCENT, width=stroke, joint="curve")
    draw.ellipse((470, 470, 554, 554), fill=FOREGROUND)

    png = image.resize((512, 512), Image.Resampling.LANCZOS)
    png.save(assets / "gradial-logo.png", optimize=True)

    icon = image.resize((256, 256), Image.Resampling.LANCZOS)
    icon.save(
        resources / "gradial.ico",
        format="ICO",
        sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


if __name__ == "__main__":
    main()
