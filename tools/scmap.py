"""Reader for Supreme Commander (FA, version 56; FAF version 60) .scmap files.

    python3 tools/scmap.py <file.scmap>      # prints a summary

Map(path) gives: size (world units), heights ((h+1) x (w+1) float array, world units), water (elevation or None),
albedo / normals (stratum texture paths and scales), decals, masks_low / masks_high (RGBA arrays of the
stratum masks 1-4 / 5-8, at their own resolution), terrain_type, props (path, position)."""
import io, struct, sys
import numpy as np


class R:
    def __init__(self, data):
        self.b, self.p = data, 0

    def take(self, n):
        v = self.b[self.p:self.p + n]
        self.p += n
        return v

    def i32(self):
        return struct.unpack_from("<i", self.take(4))[0]

    def f32(self):
        return struct.unpack_from("<f", self.take(4))[0]

    def u8(self):
        return self.take(1)[0]

    def v(self, n):
        return struct.unpack_from(f"<{n}f", self.take(4 * n))

    def cstr(self):
        e = self.b.index(b"\0", self.p)
        s = self.b[self.p:e].decode("latin1")
        self.p = e + 1
        return s


def dds_rgba(data):
    """Uncompressed 32-bit DDS (what the map stores for masks) -> HxWx4 uint8 array (R, G, B, A)."""
    if not data or data[:4] != b"DDS ":
        return None
    h, w = struct.unpack_from("<II", data, 12)
    bits = struct.unpack_from("<I", data, 88)[0]
    rmask, gmask, bmask, amask = struct.unpack_from("<IIII", data, 92)
    if bits != 32:
        from PIL import Image
        return np.asarray(Image.open(io.BytesIO(data)).convert("RGBA"))
    px = np.frombuffer(data, dtype="<u4", count=w * h, offset=128).reshape(h, w)
    out = np.zeros((h, w, 4), np.uint8)
    for i, m in enumerate((rmask, gmask, bmask, amask)):
        if m:
            shift = (m & -m).bit_length() - 1
            out[..., i] = (px & m) >> shift
    return out


class Map:
    def __init__(self, path):
        r = R(open(path, "rb").read())
        assert r.take(4) == b"Map\x1a", "not an scmap"
        r.i32(); r.i32(); r.i32()
        self.size = r.v(2)
        r.i32(); r.take(2)
        self.preview = r.take(r.i32())
        self.version = r.i32()
        w, h = r.i32(), r.i32()
        scale = r.f32()
        raw = np.frombuffer(r.take(2 * (w + 1) * (h + 1)), dtype="<u2").reshape(h + 1, w + 1)
        self.heights = raw.astype(np.float32) * scale
        if self.version >= 56:
            r.u8()
        self.shader, self.background, self.sky = r.cstr(), r.cstr(), r.cstr()
        self.cubemaps = [(r.cstr(), r.cstr()) for _ in range(r.i32())]
        r.f32(); self.sun_dir = r.v(3); r.v(3); r.v(3); r.v(3); r.v(4); r.f32(); r.v(3); r.f32(); r.f32()
        has_water = r.u8()
        elev, self.water_deep, self.water_abyss = r.f32(), r.f32(), r.f32()
        self.water = elev if has_water else None
        r.v(3); r.v(2); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.v(3); r.v(3); r.f32(); r.f32()
        r.cstr(); r.cstr()
        for _ in range(4):
            r.f32()
        for _ in range(4):
            r.v(2); r.cstr()
        for _ in range(r.i32()):
            r.cstr(); r.cstr(); r.v(3); r.f32(); r.v(3); r.v(2); r.v(2); r.v(2); r.f32(); r.v(2); r.f32()
        if self.version < 56:
            raise ValueError(f"scmap version {self.version} not supported")
        r.i32(); r.i32(); r.i32(); r.i32(); r.i32(); r.i32()  # minimap contour interval and colours
        self.albedo = [(r.cstr(), r.f32()) for _ in range(10)]
        self.normals = [(r.cstr(), r.f32()) for _ in range(9)]
        r.i32(); r.i32()
        self.decals = []
        for _ in range(r.i32()):
            did, kind, ntex = r.i32(), r.i32(), r.i32()
            tex = [r.take(r.i32()).decode("latin1") for _ in range(ntex)]
            sc, pos, rot = r.v(3), r.v(3), r.v(3)
            r.f32(); r.f32(); r.i32()
            self.decals.append(dict(kind=kind, tex=tex, scale=sc, pos=pos, rot=rot))
        for _ in range(r.i32()):
            r.i32(); r.cstr()
            for _ in range(r.i32()):
                r.i32()
        r.i32(); r.i32()
        nn = r.i32()
        for _ in range(nn):
            r.take(r.i32())
        if self.version < 56:
            r.i32()
        self.masks_low = dds_rgba(r.take(r.i32()))
        self.masks_high = dds_rgba(r.take(r.i32()))
        r.i32()
        r.take(r.i32())  # water map
        half = (w // 2) * (h // 2)
        r.take(half); r.take(half); r.take(half)  # foam, flatness, depth bias
        self.terrain_type = np.frombuffer(r.take(w * h), np.uint8).reshape(h, w)
        if self.version >= 60:
            r.v(3); r.f32(); r.v(3); r.f32(); r.f32(); r.f32(); r.f32(); r.v(3); r.v(3); r.f32(); r.f32(); r.f32()
            r.cstr(); r.cstr(); r.cstr()
            for _ in range(r.i32()):
                r.cstr(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32()
                r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.v(3)
            r.cstr(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32(); r.f32()
            r.f32(); r.f32(); r.f32(); r.f32()
        self.props = []
        for _ in range(r.i32()):
            path = r.cstr()
            pos = r.v(3)
            rx, ry, rz = r.v(3), r.v(3), r.v(3)
            sc = r.v(3)
            self.props.append(dict(path=path, pos=pos, rx=rx, rz=rz, scale=sc))
        self.left = len(r.b) - r.p


if __name__ == "__main__":
    from collections import Counter
    m = Map(sys.argv[1])
    print("version", m.version, "size", m.size, "heights", m.heights.shape, m.heights.min(), m.heights.max())
    print("water", m.water, m.water_deep, m.water_abyss, "bytes left", m.left)
    print("strata:")
    for i, (a, n) in enumerate(zip(m.albedo, m.normals + [("", 0)])):
        print(" ", i, a, n[0])
    print("masks", None if m.masks_low is None else m.masks_low.shape, None if m.masks_high is None else m.masks_high.shape)
    print("decals", len(m.decals))
    for k, c in Counter(d["tex"][0] for d in m.decals).most_common(30):
        print("  ", c, k)
    print("props", len(m.props))
    for k, c in Counter(p["path"] for p in m.props).most_common(60):
        print("  ", c, k)
