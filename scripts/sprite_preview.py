#!/usr/bin/env python3
"""Preview Tamagotchi half-block pixel art sprites in the terminal."""

# Each sprite is a grid of rows x cols. 0 = transparent.
# Positive numbers index into the palette for that state.
# Rendered using half-block characters: each char cell = 2 vertical pixels.

# Color palettes per state: index -> (r, g, b)
PALETTES = {
    "egg": {
        1: (255, 250, 230),  # cream shell
        2: (220, 200, 170),  # shell shadow
        3: (180, 220, 180),  # green spots
        4: (200, 180, 150),  # crack lines
    },
    "working": {
        1: (120, 220, 120),  # green body
        2: (80, 180, 80),    # darker green
        3: (40, 40, 40),     # eyes (dark)
        4: (255, 255, 255),  # eye highlight
        5: (255, 150, 150),  # cheeks/blush
        6: (200, 100, 80),   # mouth
        7: (100, 200, 100),  # feet
        8: (255, 220, 60),   # sparkle
    },
    "idle": {
        1: (140, 160, 200),  # blue-grey body
        2: (110, 130, 170),  # darker body
        3: (60, 60, 80),     # closed eyes
        4: (180, 190, 220),  # highlight
        5: (120, 140, 180),  # feet
        6: (200, 200, 255),  # Zzz
    },
    "input": {
        1: (255, 180, 60),   # orange body
        2: (220, 150, 40),   # darker body
        3: (40, 40, 40),     # eyes/pupils (dark)
        4: (255, 255, 255),  # eye whites
        5: (255, 60, 60),    # angry red (brows, mouth)
        6: (200, 140, 40),   # feet
        7: (255, 100, 100),  # flush/anger marks
    },
}

# ── Sprites (each row = one pixel row, rendered 2 rows per terminal line) ──

SPRITES = {
    "Egg (New)": {
        "palette": "egg",
        "grid": [
            [0,0,0,0,1,1,1,0,0,0],
            [0,0,0,1,1,1,1,1,0,0],
            [0,0,1,1,1,3,1,1,1,0],
            [0,0,1,1,1,1,1,1,1,0],
            [0,0,1,3,1,1,1,3,1,0],
            [0,0,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,1,0],
            [0,0,0,1,2,1,2,1,0,0],
            [0,0,0,0,1,1,1,0,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Working (Happy)": {
        "palette": "working",
        "grid": [
            [0,0,0,8,1,1,1,8,0,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,1,3,4,1,1,3,4,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,5,1,1,6,6,1,1,5,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,0,7,0,0,7,0,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Working Frame 2": {
        "palette": "working",
        "grid": [
            [0,0,0,1,1,1,1,0,0,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,1,1,3,1,1,3,1,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,5,1,6,1,1,6,1,5,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,7,0,0,0,0,7,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Working Frame 3": {
        "palette": "working",
        "grid": [
            [0,0,8,1,1,1,1,8,0,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,1,4,3,1,1,4,3,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,5,1,1,6,6,1,1,5,0],
            [8,1,1,1,1,1,1,1,1,8],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,0,7,0,0,7,0,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Idle (Sleeping)": {
        "palette": "idle",
        "grid": [
            [0,0,0,1,1,1,1,0,0,0],
            [0,0,1,1,1,1,1,1,0,6],
            [0,1,1,1,1,1,1,1,1,0],
            [0,1,3,3,1,1,3,3,1,6],
            [0,1,1,1,1,1,1,1,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,0,5,0,0,5,0,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Input (Angry)": {
        "palette": "input",
        "grid": [
            [0,0,0,1,1,1,1,0,0,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,1,5,1,1,1,1,5,1,0],
            [0,1,1,4,3,3,4,1,1,0],
            [0,7,1,1,1,1,1,1,7,0],
            [0,1,1,5,5,5,5,1,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,0,6,0,0,6,0,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Input Frame 2": {
        "palette": "input",
        "grid": [
            [0,0,0,1,1,1,1,0,0,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,1,1,5,1,1,5,1,1,0],
            [0,1,1,4,3,3,4,1,1,0],
            [0,7,1,1,1,1,1,1,7,0],
            [0,1,1,1,5,5,1,1,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,6,0,0,0,0,6,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
    "Input Frame 3": {
        "palette": "input",
        "grid": [
            [0,0,0,1,1,1,1,0,0,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,1,5,1,1,1,1,5,1,0],
            [0,1,1,3,4,4,3,1,1,0],
            [0,1,7,1,1,1,1,7,1,0],
            [0,1,5,1,5,5,1,5,1,0],
            [0,1,1,1,1,1,1,1,1,0],
            [0,0,1,1,1,1,1,1,0,0],
            [0,0,0,6,0,0,6,0,0,0],
            [0,0,0,0,0,0,0,0,0,0],
        ],
    },
}


def rgb_fg(r, g, b):
    return f"\033[38;2;{r};{g};{b}m"

def rgb_bg(r, g, b):
    return f"\033[48;2;{r};{g};{b}m"

RESET = "\033[0m"

def render_sprite(name, sprite_data):
    palette = PALETTES[sprite_data["palette"]]
    grid = sprite_data["grid"]
    rows = len(grid)
    cols = len(grid[0]) if rows > 0 else 0

    print(f"  {name}")
    print()

    # Render 2 pixel rows per terminal line using half-blocks
    for y in range(0, rows, 2):
        line = "  "
        for x in range(cols):
            top = grid[y][x] if y < rows else 0
            bot = grid[y + 1][x] if y + 1 < rows else 0

            if top == 0 and bot == 0:
                line += " "
            elif top == 0 and bot != 0:
                # Bottom pixel only: use lower half block with fg color
                r, g, b = palette[bot]
                line += f"{rgb_fg(r, g, b)}\u2584{RESET}"
            elif top != 0 and bot == 0:
                # Top pixel only: use upper half block with fg color
                r, g, b = palette[top]
                line += f"{rgb_fg(r, g, b)}\u2580{RESET}"
            else:
                # Both pixels: upper half = fg (top color), bg = bottom color
                tr, tg, tb = palette[top]
                br, bg_, bb = palette[bot]
                line += f"{rgb_fg(tr, tg, tb)}{rgb_bg(br, bg_, bb)}\u2580{RESET}"

        print(line)
    print()


def main():
    print()
    print("=" * 60)
    print("  TAMAGOTCHI HALF-BLOCK PIXEL ART SPRITES")
    print("=" * 60)
    print()

    for name, data in SPRITES.items():
        render_sprite(name, data)

    print("Each sprite: 10x10 pixels rendered in 10 chars x 5 lines")
    print("Uses ▀▄█ half-block characters with fg+bg colors")
    print()

if __name__ == "__main__":
    main()
