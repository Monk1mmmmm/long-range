"""Reader for Supreme Commander .scm model files (version 5)."""
import struct
import numpy as np


class Scm:
    def __init__(self, path):
        d = open(path, "rb").read()
        (magic, ver, bone_off, wbone_count, vert_off, extra_off, vert_count, idx_off, idx_count, info_off, info_count, bone_count) = struct.unpack_from("<4s11i", d, 0)
        assert magic == b"MODL", magic
        self.version = ver
        self.bones = []
        for i in range(bone_count):
            o = bone_off + i * 108
            inv = np.array(struct.unpack_from("<16f", d, o), dtype=np.float64).reshape(4, 4)
            pos = struct.unpack_from("<3f", d, o + 64)
            rot = struct.unpack_from("<4f", d, o + 76)  # w x y z
            name_off, parent, _, _ = struct.unpack_from("<4i", d, o + 92)
            end = d.index(b"\0", name_off)
            self.bones.append(dict(name=d[name_off:end].decode("latin1"), parent=parent, inv=inv, pos=pos, rot=rot))
        vt = np.dtype([("pos", "<f4", 3), ("nrm", "<f4", 3), ("tan", "<f4", 3), ("bin", "<f4", 3), ("uv", "<f4", 2), ("uv2", "<f4", 2), ("bone", "u1", 4)])
        assert vt.itemsize == 68
        self.verts = np.frombuffer(d, dtype=vt, count=vert_count, offset=vert_off)
        self.indices = np.frombuffer(d, dtype="<u2", count=idx_count, offset=idx_off).reshape(-1, 3)
        self.info = d[info_off:info_off + info_count]


if __name__ == "__main__":
    import sys
    m = Scm(sys.argv[1])
    print("version", m.version, "verts", len(m.verts), "tris", len(m.indices))
    for i, b in enumerate(m.bones):
        n = int((m.verts["bone"][:, 0] == i).sum())
        print(f"  bone {i:2} parent {b['parent']:2} verts {n:5} pos {tuple(round(x,2) for x in b['pos'])} rot {tuple(round(x,3) for x in b['rot'])} {b['name']}")
    p = m.verts["pos"]
    print("bbox", p.min(0), p.max(0))
    print("uv range", m.verts["uv"].min(0), m.verts["uv"].max(0))
