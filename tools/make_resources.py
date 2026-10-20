"""Build Windows icon and .res using only the Python standard library.

Converts the supplied 8-bit RGB/RGBA PNG into correctly sized 32-bit icon
images. Does not require Pillow, the Windows SDK, or a resource compiler.
"""
from pathlib import Path
import struct
import zlib
import math

ROOT = Path(__file__).resolve().parent.parent


def read_png(path):
    blob = path.read_bytes()
    if blob[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("Expected PNG icon")
    offset, compressed = 8, bytearray()
    while offset < len(blob):
        length = struct.unpack_from(">I", blob, offset)[0]
        kind = blob[offset + 4:offset + 8]
        data = blob[offset + 8:offset + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", data)
            if depth != 8 or color not in (2, 6) or interlace:
                raise ValueError("Icon must be non-interlaced 8-bit RGB/RGBA PNG")
        elif kind == b"IDAT":
            compressed.extend(data)
        offset += length + 12
    channels = 4 if color == 6 else 3
    raw = zlib.decompress(compressed)
    stride = width * channels
    previous = bytearray(stride)
    pixels = []
    for y in range(height):
        start = y * (stride + 1)
        mode = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for x in range(stride):
            left = row[x - channels] if x >= channels else 0
            up = previous[x]
            upper_left = previous[x - channels] if x >= channels else 0
            if mode == 1:
                prediction = left
            elif mode == 2:
                prediction = up
            elif mode == 3:
                prediction = (left + up) // 2
            elif mode == 4:
                p = left + up - upper_left
                a, b, c = abs(p - left), abs(p - up), abs(p - upper_left)
                prediction = left if a <= b and a <= c else up if b <= c else upper_left
            elif mode == 0:
                prediction = 0
            else:
                raise ValueError("Invalid PNG filter")
            row[x] = (row[x] + prediction) & 255
        pixels.append([tuple(row[x:x + channels]) + (() if channels == 4 else (255,)) for x in range(0, stride, channels)])
        previous = row
    return width, height, pixels


def icon_image(width, height, pixels, size):
    image = bytearray()
    mask_stride = ((size + 31) // 32) * 4
    mask = bytearray(mask_stride * size)
    # Box averaging with alpha-weighted color preserves the supplied artwork.
    for target_y in range(size - 1, -1, -1):
        top, bottom = target_y * height // size, max(target_y * height // size + 1, (target_y + 1) * height // size)
        for target_x in range(size):
            left, right = target_x * width // size, max(target_x * width // size + 1, (target_x + 1) * width // size)
            count = (bottom - top) * (right - left)
            rgba = [0, 0, 0, 0]
            for y in range(top, bottom):
                for x in range(left, right):
                    r, g, b, a = pixels[y][x]
                    rgba[0] += r * a
                    rgba[1] += g * a
                    rgba[2] += b * a
                    rgba[3] += a
            alpha = rgba[3] // count
            r, g, b = (value // rgba[3] if rgba[3] else 0 for value in rgba[:3])
            image.extend((b, g, r, alpha))
            if alpha == 0:
                mask[(size - 1 - target_y) * mask_stride + target_x // 8] |= 1 << (7 - target_x % 8)
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(image), 0, 0, 0, 0)
    return header + image + mask


def align(data):
    return data + b"\0" * (-len(data) % 4)


def resource(kind, name, data):
    # DWORD-aligned RES header with ordinal type/name and en-US language.
    header = struct.pack("<IIHHHHIHHII", len(data), 32, 0xFFFF, kind, 0xFFFF, name, 0, 0x1030, 0x409, 0, 0)
    return header + align(data)


def version_block(key, value=b"", children=b"", text=False):
    key_bytes = key.encode("utf-16-le") + b"\0\0"
    header = struct.pack("<HHH", 0, len(value) // 2 if text else len(value), int(text)) + key_bytes
    body = align(header) + value
    if children:
        body = align(body) + children
    return struct.pack("<H", len(body)) + body[2:]


def make_version():
    fixed = struct.pack("<13I", 0xFEEF04BD, 0x10000, 0x00010004, 0, 0x00010004, 0, 0x3F, 0, 0x40004, 1, 0, 0, 0)
    strings = b""
    for key, value in [("CompanyName", "cantonadong"), ("LegalCopyright", "Copyright (c) cantonadong"), ("FileDescription", "AudioRouter"), ("ProductName", "AudioRouter"), ("FileVersion", "1.4.0.0"), ("ProductVersion", "1.4.0.0"), ("OriginalFilename", "AudioRouter.exe")]:
        strings += align(version_block(key, (value + "\0").encode("utf-16-le"), text=True))
    table = version_block("040904B0", children=strings, text=True)
    info = version_block("StringFileInfo", children=align(table), text=True)
    translation = version_block("Translation", struct.pack("<HH", 0x409, 1200))
    var = version_block("VarFileInfo", children=align(translation), text=True)
    return version_block("VS_VERSION_INFO", fixed, align(info) + align(var))


def coff_resources(resources):
    """Write AMD64 COFF .rsrc with RVA relocations (no SDK/windres needed)."""
    tree = {}
    for kind, name, data in resources:
        tree.setdefault(kind, {})[name] = {0x409: data}
    section = bytearray()
    leaves = []

    def directory(node):
        start = len(section)
        names = sorted(node)
        section.extend(struct.pack("<IIHHHH", 0, 0, 0, 0, 0, len(names)))
        entry_start = len(section)
        section.extend(b"\0" * (8 * len(names)))
        for index, name in enumerate(names):
            value = node[name]
            if isinstance(value, dict):
                offset = directory(value) | 0x80000000
            else:
                offset = len(section)
                section.extend(b"\0" * 16)
                leaves.append((offset, value))
            struct.pack_into("<II", section, entry_start + 8 * index, name, offset)
        return start

    directory(tree)
    relocations = bytearray()
    for offset, data in leaves:
        section.extend(b"\0" * (-len(section) % 4))
        data_offset = len(section)
        struct.pack_into("<IIII", section, offset, data_offset, len(data), 0, 0)
        section.extend(data)
        relocations.extend(struct.pack("<IIH", offset, 0, 3))  # IMAGE_REL_AMD64_ADDR32NB
    symbol_offset = 60 + len(section) + len(relocations)
    header = struct.pack("<HHIIIHH", 0x8664, 1, 0, symbol_offset, 1, 0, 0)
    section_header = struct.pack("<8sIIIIIIHHI", b".rsrc\0\0\0", 0, 0, len(section), 60, 60 + len(section), 0, len(leaves), 0, 0x40300040)
    symbol = struct.pack("<8sIhHBB", b".rsrc\0\0\0", 0, 1, 0, 3, 0)
    return header + section_header + section + relocations + symbol + struct.pack("<I", 4)


def volume_pixels(level, size=256, tray=False):
    # Curved waves grow in height and bow outward, like the reference.
    # Their ends form a fan envelope; there are no radial borders or shared circles.
    waves = [(19,4.5,2.5),(20.8,8,4.5),(22.6,11.5,6.5)]
    lines = [[(4,12),(9,12),(15,6),(15,26),(9,20),(4,20),(4,12)]]
    if level == 0:
        left,height,_ = waves[0]
        right = left+2*height
        # The X starts at exactly the two endpoints of the smallest wave.
        lines += [[(left,16-height),(right,16+height)],
                  [(left,16+height),(right,16-height)]]
    segments = [(a,b) for line in lines for a,b in zip(line,line[1:])]
    pixels = []
    for y in range(size):
        row = []
        for x in range(size):
            px, py = (x+0.5)*32/size, (y+0.5)*32/size
            if tray:
                # Fit the full stroked outline (x=3.2..29.9) inside a
                # two-unit transparent border. Center on the artwork, not
                # the canvas; stretching the old artwork left almost no
                # left margin and clipped the tallest wave vertically.
                tray_scale = 28 / (29.9 - 3.2)
                px = (px-16)/tray_scale+(3.2+29.9)/2
                py = (py-16)/tray_scale+16
            distance = 1000
            for (ax,ay),(bx,by) in segments:
                dx,dy = bx-ax,by-ay
                t = max(0,min(1,((px-ax)*dx+(py-ay)*dy)/(dx*dx+dy*dy)))
                distance = min(distance,((px-ax-t*dx)**2+(py-ay-t*dy)**2)**0.5)
            coverage = max(0,min(1,(0.8-distance)*size/32+0.5))
            if level:
                for index,(left,height,bulge) in enumerate(waves):
                    # More bow on the longer waves creates a clear fan-shaped envelope.
                    t = max(-height,min(height,py-16))
                    for _ in range(4):
                        curve_x = left+bulge*(1-(t/height)**2)
                        slope = -2*bulge*t/(height*height)
                        curvature = -2*bulge/(height*height)
                        gradient = (curve_x-px)*slope+t-(py-16)
                        derivative = slope*slope+(curve_x-px)*curvature+1
                        if abs(derivative) > 0.001:
                            t = max(-height,min(height,t-gradient/derivative))
                    ax,ay = left+bulge*(1-(t/height)**2),16+t
                    wave_distance = math.hypot(px-ax,py-ay)
                    if index < level:
                        wave_coverage = max(0,min(1,(0.8-wave_distance)*size/32+0.5))
                    else:
                        # Keep the complete solid outline in a secondary shade.
                        wave_coverage = max(0,min(1,(0.8-wave_distance)*size/32+0.5))*0.26
                    coverage = max(coverage,wave_coverage)
            alpha = int(coverage*255)
            row.append((255,255,255,alpha))
        pixels.append(row)
    return size,size,pixels


def status_images():
    width, height, pixels = read_png(ROOT / "pic/status.png")
    # The supplied sheet has four states separated by transparent gutters.
    boundaries = [0, round(width * 355 / 1511), round(width * 750 / 1511),
                  round(width * 1148 / 1511), width]
    images = []
    for left, right in zip(boundaries, boundaries[1:]):
        occupied = [(x, y) for y in range(height) for x in range(left, right)
                    if pixels[y][x][3]]
        if not occupied:
            raise ValueError("Status sheet contains an empty state")
        x0 = min(x for x, y in occupied)
        x1 = max(x for x, y in occupied) + 1
        y0 = min(y for x, y in occupied)
        y1 = max(y for x, y in occupied) + 1
        # Reserve at least one transparent pixel per side at 16px display size.
        size = math.ceil(max(x1-x0, y1-y0) * 8 / 7)
        canvas = [[(0, 0, 0, 0) for _ in range(size)] for _ in range(size)]
        dx, dy = (size-(x1-x0)) // 2, (size-(y1-y0)) // 2
        for y in range(y0, y1):
            canvas[dy+y-y0][dx:dx+x1-x0] = pixels[y][x0:x1]
        images.append((size, size, canvas))
    return images


def main():
    width, height, pixels = read_png(ROOT / "pic/ico.png")
    sizes = [16, 24, 32, 48, 64, 128, 256]
    images = [icon_image(width, height, pixels, size) for size in sizes]
    entries, group = bytearray(), bytearray(struct.pack("<HHH", 0, 1, len(images)))
    image_offset = 6 + len(images) * 16
    resource_items = []
    for ordinal, (size, image) in enumerate(zip(sizes, images), 1):
        dimension = size if size < 256 else 0
        entries.extend(struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(image), image_offset))
        group.extend(struct.pack("<BBBBHHIH", dimension, dimension, 0, 0, 1, 32, len(image), ordinal))
        resource_items.append((3, ordinal, image))
        image_offset += len(image)
    resource_items.append((14, 1, group))
    ordinal = len(images) + 1
    state_images = status_images()
    for level in range(4):
        vw,vh,vpixels = state_images[level]
        state_icon = icon_image(vw,vh,vpixels,128)
        (ROOT / f"resources/volume{level}.bin").write_bytes(state_icon)
        (ROOT / f"resources/tray-volume{level}.bin").write_bytes(state_icon)
        volume_group = bytearray(struct.pack("<HHH", 0, 1, 4))
        for size in [16,24,32,48]:
            image = icon_image(vw,vh,vpixels,size)
            resource_items.append((3, ordinal, image))
            volume_group.extend(struct.pack("<BBBBHHIH", size,size,0,0,1,32,len(image),ordinal))
            ordinal += 1
        resource_items.append((14, level+2, volume_group))
    resource_items.append((24, 1, (ROOT / "resources/app.manifest").read_bytes()))
    resource_items.append((16, 1, make_version()))
    res = bytearray(struct.pack("<IIHHHHIHHII", 0, 32, 0xFFFF, 0, 0xFFFF, 0, 0, 0, 0, 0, 0))
    for kind, name, data in resource_items:
        res.extend(resource(kind, name, data))
    (ROOT / "resources/app.ico").write_bytes(struct.pack("<HHH", 0, 1, len(images)) + entries + b"".join(images))
    (ROOT / "resources/app.res").write_bytes(res)
    (ROOT / "resources/app.o").write_bytes(coff_resources(resource_items))
    print(f"Icon/resource ready: {width}x{height}, {len(images)} icon sizes")


if __name__ == "__main__":
    main()
