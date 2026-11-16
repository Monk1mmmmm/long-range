"""Reader for Supreme Commander .sca animation files (version 5).

Each frame holds, per animated bone, its local transform relative to the parent bone: position (x, y, z)
and rotation quaternion (w, x, y, z), the same convention as the .scm rest pose (local = T(pos) R(rot)).
The mod's exporter wrote 0 for every frame time, so frames are taken as evenly spaced over the duration
(the last frame loops back to the first)."""
import struct


class Sca:
    def __init__(self, path):
        """`path`: a file path or the file's bytes."""
        d = path if isinstance(path, (bytes, bytearray)) else open(path, "rb").read()
        magic, ver, nframes, dur, nbones, names_off, links_off, first_off, fsize = struct.unpack_from("<4siifiiiii", d, 0)
        assert magic == b"ANIM", magic
        self.version = ver
        self.duration = dur
        self.names = [n.decode("latin1") for n in d[names_off:links_off].split(b"\0")[:nbones]]
        self.parents = list(struct.unpack_from(f"<{nbones}i", d, links_off))
        # Root motion of the whole animation (position + rotation), unused for walk cycles.
        self.root = struct.unpack_from("<7f", d, first_off)
        self.frames = []
        o = first_off + 28
        for _ in range(nframes):
            t, flags = struct.unpack_from("<fI", d, o)
            bones = [(struct.unpack_from("<3f", d, o + 8 + 28 * i), struct.unpack_from("<4f", d, o + 20 + 28 * i)) for i in range(nbones)]
            self.frames.append(dict(time=t, flags=flags, bones=bones))
            o += fsize

    def times(self):
        """Frame times: the stored ones when they increase, else evenly spaced over the duration."""
        ts = [f["time"] for f in self.frames]
        if all(b > a for a, b in zip(ts, ts[1:])):
            return ts
        n = len(self.frames)
        return [self.duration * k / n for k in range(n)]


if __name__ == "__main__":
    import sys
    a = Sca(sys.argv[1])
    print(f"version {a.version} frames {len(a.frames)} duration {a.duration}")
    for i, (n, p) in enumerate(zip(a.names, a.parents)):
        print(f"  bone {i:2} parent {p:2} {n}")
