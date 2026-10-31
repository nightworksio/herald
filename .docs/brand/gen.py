#!/usr/bin/env python3
import os

HERE = os.path.dirname(os.path.abspath(__file__))

PALETTE = {
    "k": "#0b0914",
    "s": "#12101f",
    "n": "#2b2638",
    "m": "#36314b",
    "e": "#3a3450",
    "M": "#443f5a",
    "g": "#5a5470",
    "G": "#7a7392",
    "h": "#9b9bb0",
    "H": "#c6c6d6",
    "*": "#f4f1c9",
    "+": "#8f88c7",
    "1": "#4f2c12",
    "2": "#8c5418",
    "3": "#e39a26",
    "4": "#ffc94a",
    "5": "#fff0b8",
    "R": "#7a2219",
    "r": "#c43c28",
    "o": "#ee6a3f",
    "O": "#ffa070",
    "w": "#f2eedc",
    "p": "#d89aa8",
    "V": "#8f88c7",
    "v": "#5a4f9a",
}

CRAB = """
................................
................................
................................
................................
.O..O...........................
.Oo.oO..........................
.oo.or..........................
.ooooR..........................
.rooRR..........................
..rRR..ww....ww.............44..
..rR..wwkw..wwkw...........4454.
...rr.wwkw..wwkw..........43115.
....rr.ww....ww...........42115.
.....r..R....R............42115.
.....r..R....R.........44432114.
.......OOOOOOOO.....44422222114.
.....OOOooooookkk444222..3.2234.
....OOooooookk444222444444..33..
...rrpprrrrk44222kk.VVVVVv......
...rrrrrrrrk22kOkOk.VV4VVv......
...rrrrrrrrrrrkOook.V444Vv......
...rrrrr444VVVrkork.VV4VVv......
..r.rrrr444VVVrrkR.rVVVVVv......
.R..rrrr333vvvrrrR..VVVVVv......
.R.r.RRRVVV444RRR.r.VVVVVv......
..R..r.RVVV444R.r..RVv..Vv......
..R.R..kvvv333k..R.R............
....R..kkkkkkkk..R..............
"""

CRAB_HI = """
..................................................
..k...............................................
.kOk...k..........................................
kOOok.kOk.........................................
kOookkOOok........................................
koookkoork........................................
koookkooRk........................................
kooooooorRk.......................................
kroooooorRk.......................................
.krooooRRk....................................k...
..krrRRRk....................................k4k..
...krRRk....................................k454k.
...krRk...kkk........kkk....................k415k.
...krRk..kwwwk......kwwwk..................k4315k.
....krRkkwwwwwk....kwwwwwk.................k4315k.
....krRkkwwkkwk....kwwkkwk................k44315k.
.....krRkwwkkwk....kwwkkwk................k43215k.
.....krRkwwkkwk....kwwkkwk...............k443215k.
......krRkwwwk......kwwwk..............kk4433215k.
.......krRkRk........kRk..............k443332215k.
.......krRkRk........kRk...........kkk4332222114k.
........krRRk........kRk........kkk5443222222114k.
........krRRkkkkkkkkkkRk.....kkk444333222kk22114k.
........kkkOOOOOOOOOOOOkkkkkk454333222kkk..k2214k.
.......kOOoooooooooooooooo444333222kkk.....k2214k.
......kOOoooooooooooooo445333222kkkk3k......k214k.
.....kOOrrrrrrrrrrrr544333222k3kkkkk3k......k223k.
....kOOrrrrrrrrrk444333222rrOk44444442k......k3k..
....krrrrpprrrppk33322kOkkkOkVVVVVVVVvk.......k...
....krrrrrrrrrrrk222rrkOokooRVVVVVVVVvk...........
....krrrrrrrrrrrrrrrrrkooooRkVVVV4VVVvk...........
....krrrrrrrrrrrrrrrrrrkroRRkVVVV4VVVvk...........
....krrrrrrr55555VVVVVrrkrRkRVV44544Vvk...........
...orrrrrrrr44444VVVVVrrrrrrRVVVV4VVVvk...........
..oRkrrrrrrr44444VVVVVrrrrrrRVVVV4VVVvk...........
.rR..krrrrrr44444VVVVVrrrrrR.VVVVVVVVvk...........
.R...orRRRRR33333vvvvvRRRRRroVVVVVVVVvk...........
R...oR.kRRRRVVVVV44444RRRR...VVVVkVVVvk...........
...rR...orkkVVVVV44444kkro...VVVk.kVVvk...........
...R...oR..kVVVVV44444k..Ro..3vk...kv3k...........
..R...rR...kVVVVV44444k...Rr......................
..R...R....kvvvvv33333k....R......................
.....R......kkkkkkkkkk......R.....................
.....R......................R.....................
"""

SOUND = """
......33....
........3...
...44....3..
.....4...3..
55...4....3.
..5...4...3.
..5...4...3.
..5...4...3.
..5...4...3.
55...4....3.
.....4...3..
...44....3..
........3...
......33....
"""

NOTE = """
.55
.4.
44.
44.
"""

NOTE_SMALL = """
.5
54
4.
"""

BRAZIER = """
..4...
..43..
.3454.
.4553.
.3553.
khhhhk
kgGGgk
.kggk.
..Mk..
..Mk..
.MMMk.
"""

GLYPHS = {
    "h": ["X..", "X..", "XXX", "X.X", "X.X"],
    "e": ["...", "XXX", "XXX", "X..", "XXX"],
    "r": ["...", "XXX", "X..", "X..", "X.."],
    "a": ["...", ".XX", "X.X", "X.X", ".XX"],
    "l": ["X.", "X.", "X.", "X.", "XX"],
    "d": ["..X", ".XX", "X.X", "X.X", ".XX"],
}


def parse(art):
    return art.strip("\n").split("\n")


class Canvas:
    def __init__(self, w, h, fill="s"):
        self.w, self.h = w, h
        self.px = [[PALETTE[fill]] * w for _ in range(h)]

    def set(self, x, y, ch):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = PALETTE[ch]

    def rect(self, x, y, w, h, ch):
        for yy in range(y, y + h):
            for xx in range(x, x + w):
                self.set(xx, yy, ch)

    def blit(self, art, x0, y0):
        for dy, row in enumerate(parse(art)):
            for dx, ch in enumerate(row):
                if ch != ".":
                    self.set(x0 + dx, y0 + dy, ch)

    def dots(self, points):
        for x, y, ch in points:
            self.set(x, y, ch)

    def to_svg(self, cell, title):
        w, h = self.w * cell, self.h * cell
        out = [
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" shape-rendering="crispEdges">',
            f"<title>{title}</title>",
        ]
        for x, y, rw, rh, c in merge(self.px):
            out.append(
                f'<rect x="{x * cell}" y="{y * cell}" width="{rw * cell}" height="{rh * cell}" fill="{c}"/>'
            )
        out.append("</svg>")
        return "\n".join(out) + "\n"


def merge(px):
    h, w = len(px), len(px[0])
    done = [[False] * w for _ in range(h)]
    rects = []
    for y in range(h):
        x = 0
        while x < w:
            if done[y][x]:
                x += 1
                continue
            c = px[y][x]
            run = 1
            while x + run < w and px[y][x + run] == c and not done[y][x + run]:
                run += 1
            depth = 1
            while y + depth < h and all(
                px[y + depth][xx] == c and not done[y + depth][xx] for xx in range(x, x + run)
            ):
                depth += 1
            for yy in range(y, y + depth):
                for xx in range(x, x + run):
                    done[yy][xx] = True
            rects.append((x, y, run, depth, c))
            x += run
    return rects


def bricks(c, x0, y0, w, h, phase=0):
    for y in range(y0, y0 + h):
        row = (y - y0) % 4
        course = (y - y0) // 4
        for x in range(x0, x0 + w):
            if row == 3:
                ch = "m"
            elif (x - x0 + phase + (4 if course % 2 else 0)) % 8 == 7:
                ch = "m"
            elif row == 0:
                ch = "G"
            else:
                ch = "g"
            c.set(x, y, ch)


def text_width(word, f):
    return sum(len(GLYPHS[ch][0]) * f for ch in word) + (len(word) - 1) * f


def wordmark(c, word, x0, y0, f, shadow):
    def paint(ox, oy, shadow_pass):
        x = x0 + ox
        for ch in word:
            glyph = GLYPHS[ch]
            gw = len(glyph[0])
            for gy, row in enumerate(glyph):
                for gx, bit in enumerate(row):
                    if bit != "X":
                        continue
                    for yy in range(f):
                        py = gy * f + yy
                        if shadow_pass:
                            colour = "k"
                        elif gy == 0:
                            colour = "5"
                        elif py < f + 2 * f:
                            colour = "4"
                        else:
                            colour = "3"
                        for xx in range(f):
                            c.set(x + gx * f + xx, y0 + oy + py, colour)
            x += gw * f + f

    paint(shadow, shadow, True)
    paint(0, 0, False)


def write(name, canvas, cell, title):
    with open(os.path.join(HERE, name), "w") as f:
        f.write(canvas.to_svg(cell, title))


def mark():
    c = Canvas(32, 32)
    c.dots([(2, 1, "*"), (15, 5, "*"), (18, 0, "+"), (29, 24, "*")])
    c.blit(CRAB, 0, 0)
    c.blit(NOTE, 26, 3)
    c.blit(NOTE_SMALL, 21, 1)
    bricks(c, 0, 28, 32, 4, phase=2)
    return c


def logo():
    c = Canvas(72, 64)
    c.dots([
        (3, 2, "+"), (24, 6, "*"), (21, 1, "+"), (30, 3, "*"), (2, 21, "+"), (17, 9, "+"),
        (38, 1, "+"), (47, 4, "*"), (58, 2, "+"), (68, 6, "*"), (69, 34, "*"), (40, 8, "*"),
    ])
    wall_top = 47
    sx, sy = 3, wall_top - 44
    bricks(c, 0, wall_top, 72, 4)
    c.rect(0, 51, 72, 1, "e")
    c.rect(0, 52, 72, 12, "n")
    c.blit(CRAB_HI, sx, sy)
    c.blit(BRAZIER, 62, wall_top - 11)
    c.blit(SOUND, sx + 50, sy + 12)
    for nx, ny, art in [(sx + 54, sy + 2, NOTE), (sx + 60, sy + 9, NOTE_SMALL), (sx + 63, sy + 24, NOTE),
                        (sx + 44, sy + 0, NOTE_SMALL)]:
        c.blit(art, nx, ny)
    f = 2
    tw = text_width("herald", f)
    wordmark(c, "herald", (72 - tw) // 2, 53, f, 1)
    return c


def social():
    c = Canvas(128, 64)
    c.dots([
        (3, 3, "+"), (11, 1, "+"), (24, 2, "+"), (33, 3, "*"), (41, 1, "+"), (46, 0, "*"),
        (57, 2, "*"), (70, 6, "*"), (84, 2, "+"), (97, 9, "*"), (110, 3, "+"), (122, 7, "*"),
        (118, 17, "*"), (124, 25, "+"), (91, 52, "+"), (104, 21, "+"),
    ])
    wall_top = 48
    sx, sy = 1, wall_top - 44
    bricks(c, 0, wall_top, 54, 8)
    c.blit(BRAZIER, 46, wall_top - 11)
    c.blit(CRAB_HI, sx, sy)
    c.blit(SOUND, sx + 50, sy + 12)
    for nx, ny, art in [(sx + 53, sy + 2, NOTE), (sx + 44, sy + 0, NOTE_SMALL), (sx + 65, sy + 10, NOTE_SMALL),
                        (sx + 74, sy + 1, NOTE), (sx + 86, sy + 13, NOTE_SMALL)]:
        c.blit(art, nx, ny)
    c.rect(0, 56, 128, 1, "e")
    c.rect(0, 57, 128, 7, "n")
    for y in (59, 62):
        for x in range(1 if y == 59 else 3, 128, 5):
            c.set(x, y, "m")
    f = 3
    tw = text_width("herald", f)
    x0 = 125 - tw
    wordmark(c, "herald", x0, 31, f, 1)
    for x in range(x0, x0 + tw, 3):
        c.rect(x, 51, 2, 1, "2")
    return c


if __name__ == "__main__":
    write("herald-mark.svg", mark(), 8, "herald")
    write("herald.svg", logo(), 8, "herald")
    write("herald-social.svg", social(), 10, "herald")
