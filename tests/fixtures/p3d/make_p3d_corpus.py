#!/usr/bin/env python3
"""Generate the synthetic P3D/MLOD test corpus (FIXTURE-P3D-1).

The corpus the P3D tests were written against was licensed Bohemia data, and the
source release omits it (see tests/fixtures/ASSET_SOURCES.md). What is left in the
tests is a set of *structural* contracts -- how many LODs, which LOD carries the
proxies, where the section byte offsets land, how a quad survives ODOL and SP3X,
what a TAGG selection looks like -- wrapped around literals taken from the removed
models (their coordinates, texture paths, Czech selection names).

This script authors that structure from scratch, so the contracts stay testable:

* geometry is a deterministic spiral lattice, not anybody's mesh;
* texture and material paths are `data\\synthetic\\...`;
* selection and proxy names are generic roles (`rotor_main`, `proxy:cargo.01`);
* counts the tests document as invariants (19 LODs, 44 component selections,
  6037-point proxy LOD, ...) are reproduced, because those numbers *are* the
  contract being asserted.

Both encodings are emitted from one model description, so
`p3d/complex_vehicle.p3d` and `mlod/complex_vehicle_mlod.p3d` are the same model
compiled twice -- which is what the MLOD/ODOL parity tests exist to check.

Wire layouts: engine/Poseidon/Asset/Formats/P3D/P3DStructures.hpp (ODOL v7) and
.../MLODStructures.hpp (MLOD 1.1 + SP3X). Arrays of >= 1024 bytes are LZSS
compressed (BISBinaryStream.hpp: COMPRESSION_THRESHOLD), so `lzss_literals()`
below emits a valid stream made entirely of literal bytes and its trailing
checksum -- legal input that needs no real compressor.

Regenerate:  python tests/fixtures/p3d/make_p3d_corpus.py
"""
from __future__ import annotations

import math
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent
MLOD_DIR = HERE.parent / "mlod"

COMPRESSION_THRESHOLD = 1024
BS = chr(92)  # a single backslash, without a quoting layer eating it


# ── primitives ────────────────────────────────────────────────────────────────

def p(fmt: str, *values: object) -> bytes:
    return struct.pack("<" + fmt, *values)


def asciiz(text: str) -> bytes:
    # P3DM/SP3X/ODOL all carry latin1 file paths.
    return text.encode("latin1") + b"\0"


def lzss_literals(data: bytes) -> bytes:
    """A valid LZSS stream made of literal bytes only, plus its checksum.

    Decoder (SsCompress::Decode): one flag byte per eight items, lsb first; bit
    set = literal byte, bit clear = two-byte back-reference. Every item here is a
    literal, so the stream round-trips without a match finder.
    """
    out = bytearray()
    checksum = 0
    for start in range(0, len(data), 8):
        group = data[start:start + 8]
        out.append(0xFF)  # all eight items are literals
        out += group
        checksum += sum(group)
    out += struct.pack("<i", checksum)
    return bytes(out)


def compressed_array(width: int, values) -> bytes:
    """count + payload, LZSS-compressed once the payload reaches the threshold."""
    raw = b"".join(values)
    out = p("I", len(raw) // width)
    if len(raw) >= COMPRESSION_THRESHOLD:
        return out + lzss_literals(raw)
    return out + raw


def u16_array(values) -> bytes:
    return compressed_array(2, [p("H", v) for v in values])


def u8_array(values) -> bytes:
    return compressed_array(1, [p("B", v) for v in values])


def u32_array(values) -> bytes:
    return compressed_array(4, [p("I", v) for v in values])


def raw_u32_array(values) -> bytes:
    """readArray<T>: count then raw bytes, never compressed."""
    return p("I", len(values)) + b"".join(p("I", v) for v in values)


def vec2_array(values) -> bytes:
    return compressed_array(8, [p("ff", u, v) for u, v in values])


def vec3_array(values) -> bytes:
    return p("I", len(values)) + b"".join(p("fff", *v) for v in values)


def f32_array(values) -> bytes:
    return compressed_array(4, [p("f", v) for v in values])


# ── synthetic geometry ───────────────────────────────────────────────────────

def spiral(count: int, extent: float, normals: int = 0):
    """`count` deterministic points on a sphere, plus unit normals and UVs.

    A golden-angle spiral spreads the points evenly without any per-shape special
    casing, so a 2-point camera helper and a 6037-point structure use one rule.
    """
    points, uvs = [], []
    for i in range(count):
        t = 0.0 if count == 1 else i / (count - 1)
        angle = i * 2.399963229728653
        y = 1.0 - 2.0 * t
        radius = math.sqrt(max(0.0, 1.0 - y * y))
        points.append((extent * radius * math.cos(angle), extent * y, extent * radius * math.sin(angle)))
        uvs.append(((i % 7) / 6.0, t))
    return points, [normal(i) for i in range(normals or count)], uvs


def column(count: int, radius: float, height: float):
    """`count` points on a tall cylindrical shell standing on y = 0.

    A structure, not a ball: its xz footprint is `2 * radius` across while its
    height is `height`, so a low sun throws a long shadow and an overhead sun a
    footprint-sized one. That ratio is what the shadow probe asserts -- a sphere
    casts the same silhouette at every elevation, so it cannot show the light
    direction reaching the shadow matrices at all.
    """
    points, uvs = [], []
    for i in range(count):
        t = 0.0 if count == 1 else i / (count - 1)
        angle = i * 2.399963229728653
        points.append((radius * math.cos(angle), height * t, radius * math.sin(angle)))
        uvs.append(((i % 7) / 6.0, t))
    return points, [normal(i) for i in range(count)], uvs


def normal(index: int):
    """Normals cannot all be the spiral's own direction (they must be independent
    data the reader re-reads), so vary them slightly but keep them unit length."""
    rix = index * 0.6180339887
    theta = (rix % 1.0) * 2.0 * math.pi
    y = -1.0 + 2.0 * ((index * 0.7548776662) % 1.0)
    r = math.sqrt(max(0.0, 1.0 - y * y))
    return (r * math.cos(theta), y, r * math.sin(theta))


def topology(points, triangle_count, quad_count, texture_for, groups=1):
    """Faces over consecutive points, mixing triangles and quads.

    Face i takes `texture_for(i)`; the loader reverses the first four indices on
    load, which is the documented ODOL winding convention. `groups` splits the
    faces into that many material runs, because an ODOL section is one run of a
    single (texture, material) pair and the tests count sections.
    """
    total = triangle_count + quad_count
    faces = []
    cursor = 0
    for i in range(total):
        quad = i >= triangle_count
        width = 4 if quad else 3
        indices = [(cursor + k) % len(points) for k in range(width)]
        cursor += 1
        material = (i * groups) // max(1, total)
        faces.append((indices, texture_for(i), material))
    return faces


def bounds_of(points):
    mn = tuple(min(pt[i] for pt in points) for i in range(3))
    mx = tuple(max(pt[i] for pt in points) for i in range(3))
    center = tuple((mn[i] + mx[i]) * 0.5 for i in range(3))
    radius = 0.0
    for pt in points:
        radius = max(radius, math.dist(center, pt))
    return mn, mx, center, radius


# ── ODOL v7 ──────────────────────────────────────────────────────────────────

class Lod:
    """One LOD as the tests describe it, independent of either wire encoding."""

    def __init__(self, points, normals, uvs, faces, textures,
                 selections=None, properties=None, frames=None, proxies=None,
                 resolution=1.0, colors=(0xFF808080, 0xFF808080, 0), or_hints=0, and_hints=0,
                 mlod_edges=None, vertex_edges=None):
        self.points = points
        self.normals = normals
        self.uvs = uvs
        self.faces = faces
        self.textures = textures
        self.selections = selections or []
        self.properties = properties or []
        self.frames = frames or []
        self.proxies = proxies or []
        self.resolution = resolution
        self.colors = colors
        self.or_hints = or_hints
        self.and_hints = and_hints
        self.mlod_edges = mlod_edges or []
        self.vertex_edges = vertex_edges or []


def odol_face_record(indices, texture_index, flags=0):
    body = p("IhB", flags, texture_index, len(indices)) + b"".join(p("H", i) for i in indices)
    return body


def write_odol_lod(lod: Lod) -> bytes:
    data = bytearray()
    # Vertex table.
    data += compressed_array(4, [p("I", 0) for _ in lod.points])
    data += vec2_array(lod.uvs)
    data += vec3_array(lod.points)
    data += vec3_array(lod.normals)

    # Bounds.
    mn, mx, center, radius = bounds_of(lod.points) if lod.points else ((0.0,) * 3, (0.0,) * 3, (0.0,) * 3, 0.0)
    data += p("ii", lod.or_hints, lod.and_hints)
    data += p("fff", *mn) + p("fff", *mx) + p("fff", *center) + p("f", radius)

    # Textures.
    data += p("I", len(lod.textures))
    for texture in lod.textures:
        data += asciiz(texture)

    # Edges (mlod index, then vertex index).
    data += u16_array(lod.mlod_edges)
    data += u16_array(lod.vertex_edges)

    # Faces: the section table is addressed in *bytes*, so measure as we go.
    face_bytes = bytearray()
    offsets = [0]
    for indices, texture, _material in lod.faces:
        texture_index = texture if isinstance(texture, int) else (-1 if texture == "" else lod.textures.index(texture))
        face_bytes += odol_face_record(indices, texture_index)
        offsets.append(len(face_bytes))
    data += p("I", len(lod.faces)) + p("I", len(face_bytes))
    data += face_bytes

    # Sections: one per run of an identical (textureIndex, material) pair.
    runs = []
    for face_index, (_indices, texture, material) in enumerate(lod.faces):
        texture_index = texture if isinstance(texture, int) else (-1 if texture == "" else lod.textures.index(texture))
        if runs and runs[-1][2] == texture_index and runs[-1][3] == material:
            runs[-1] = (runs[-1][0], face_index + 1, texture_index, material)
        else:
            runs.append((face_index, face_index + 1, texture_index, material))
    data += p("I", len(runs))
    for first, last, texture_index, material in runs:
        data += p("IIihI", offsets[first], offsets[last], 0 if material is None else material,
                  texture_index, 0)

    # Named selections.
    data += p("I", len(lod.selections))
    for name, point_indices, face_indices in lod.selections:
        data += asciiz(name)
        data += u16_array(sorted(face_indices))
        data += u8_array([255] * len(face_indices))
        data += u32_array(list(range(len(sorted(face_indices)))))
        data += p("B", 0)
        data += u32_array([])
        data += u16_array(sorted(point_indices))
        data += u8_array([255] * len(point_indices))

    # Named properties.
    data += p("I", len(lod.properties))
    for name, value in lod.properties:
        data += asciiz(name) + asciiz(value)

    # Frames.
    data += p("I", len(lod.frames))
    for time, positions in lod.frames:
        data += p("f", time) + p("I", len(positions)) + b"".join(p("fff", *pos) for pos in positions)

    # LOD end data.
    data += p("III", *lod.colors)

    # Proxies.
    data += p("I", len(lod.proxies))
    for name, translation, sequence_id, selection_index in lod.proxies:
        data += asciiz(name)
        data += p("fff", 1.0, 0.0, 0.0)
        data += p("fff", 0.0, 1.0, 0.0)
        data += p("fff", 0.0, 0.0, 1.0)
        data += p("fff", *translation)
        data += p("I", sequence_id) + p("I", selection_index)

    return bytes(data)


def write_odol(path: Path, lods, meta) -> None:
    data = bytearray(b"ODOL" + p("I", 7) + p("I", len(lods)))
    for lod in lods:
        data += write_odol_lod(lod)
    for lod in lods:
        data += p("f", lod.resolution)

    every_point = [pt for lod in lods for pt in lod.points]
    mn, mx, center, _radius = bounds_of(every_point) if every_point else ((0.0,) * 3, (0.0,) * 3, (0.0,) * 3, 0.0)
    radius = meta.get("radius") or max((math.dist(center, pt) for pt in every_point), default=0.0)
    data += p("i", 0)
    data += p("f", radius)
    data += p("f", radius)
    data += p("iii", 0, 0, 0)
    data += p("fff", *center)
    data += p("II", meta.get("color", 0xFFFFFFFF), meta.get("colorTop", 0xFFFFFFFF))
    data += p("f", 1.0)
    data += p("fff", *mn) + p("fff", *mx)
    data += p("fff", *center) + p("fff", *center) + p("fff", 0.0, 0.0, 0.0)
    data += p("fffffffff", 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
    data += p("BBBBB", 0, 0, 0, 0, 1)
    data += p("b", 0)
    data += f32_array(meta.get("mass", []))
    data += p("ffff", meta.get("mass_total", 0.0), 0.0, 0.0, 0.0)
    data += p("b" * 12, *meta.get("lod_indices", [-1] * 12))

    path.write_bytes(bytes(data))


# ── MLOD 1.1 / SP3X ──────────────────────────────────────────────────────────

def write_sp3x_lod(lod: Lod) -> bytes:
    normals = lod.normals
    # SP3X stores one normal per face corner, not one per point.
    face_corners = sum(len(indices) for indices, _t, _m in lod.faces)
    normals = [normal(i) for i in range(max(face_corners, len(lod.normals)))]

    data = bytearray()
    data += b"SP3X" + p("iiiiii", 28, 1, len(lod.points), len(normals), len(lod.faces), 0)
    for index, pt in enumerate(lod.points):
        data += p("fff", *pt) + p("i", 0)
    for norm in normals:
        data += p("fff", *norm)

    cursor = 0
    for indices, texture, _material in lod.faces:
        path = texture if isinstance(texture, str) else (lod.textures[texture] if lod.textures else "")
        field = path.encode("latin1")[:32].ljust(32, b"\0")
        data += field
        data += p("i", len(indices))
        for slot in range(4):
            if slot < len(indices):
                data += p("iiff", indices[slot], cursor, (slot % 2) * 1.0, (slot // 2) * 1.0)
                cursor = (cursor + 1) % max(1, len(normals))
            else:
                data += p("iiff", 0, 0, 0.0, 0.0)
        data += p("i", 0)

    # TAGG: fixed 64-byte name + int32 size, per MLODStructures.hpp.
    data += b"TAGG"
    for name, point_indices, face_indices in lod.selections:
        payload = bytes(1 if i in point_indices else 0 for i in range(len(lod.points)))
        payload += bytes(1 if i in face_indices else 0 for i in range(len(lod.faces)))
        data += name.encode("latin1").ljust(64, b"\0") + p("i", len(payload)) + payload
    for name, value in lod.properties:
        data += b"#Property#".ljust(64, b"\0") + p("i", 128)
        data += name.encode("latin1").ljust(64, b"\0") + value.encode("latin1").ljust(64, b"\0")
    data += b"#EndOfFile#".ljust(64, b"\0") + p("i", 0)
    data += p("f", lod.resolution)
    return bytes(data)


def write_mlod(path: Path, lods) -> None:
    data = bytearray(b"MLOD" + p("BBHI", 1, 1, 0, len(lods)))
    for lod in lods:
        data += write_sp3x_lod(lod)
    path.write_bytes(bytes(data))


# ── the corpus ───────────────────────────────────────────────────────────────

def synthetic_texture(index: int) -> str:
    return f"data{BS}synthetic{BS}surface_{index:02d}.pac"


def role_selections(count: int):
    """Generic role names, with proxy selections at the indices the tests use.

    Indices 9..22 and 29 are proxies (15 of them, matching the 15 proxy records),
    everything else is a plain part. `proxy:` prefixes are the engine's own
    convention for binding a proxy to a named selection, and the `.<NN>` suffix
    is the instance id. The four names the tests assert by index are pinned here
    so a change of convention is a change of one table.
    """
    proxy_slots = list(range(9, 23)) + [29]
    pinned = {9: "proxy:crew.01", 10: "proxy:cargo.01", 14: "proxy:cargo.04", 29: "proxy:flag_alone.01"}
    names = dict(pinned)
    instance = 0
    for slot in proxy_slots:
        if slot not in names:
            names[slot] = f"proxy:cargo.{(instance % 13) + 2:02d}"
            instance += 1
    for slot in range(count):
        if slot not in names:
            names[slot] = f"part_{slot + 1:02d}"
    return names


def vehicle_lods() -> list[Lod]:
    selections = role_selections(30)
    proxy_order = [9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 29]
    proxy_names = ["crew", "cargo", "cargo", "cargo", "cargo", "cargo",
                   "cargo", "cargo", "cargo", "cargo", "cargo", "cargo",
                   "cargo", "cargo", "flag_alone"]

    lods = []
    counts = [2617, 1905, 1700, 1500, 1300, 1100, 950, 820, 700, 600,
              520, 460, 394]
    for index, count in enumerate(counts):
        points, normals, uvs = spiral(count, 9.0)
        if index == 0:
            faces = topology(points, 549, 792, lambda i: synthetic_texture(0), groups=101)
            textures = [synthetic_texture(i) for i in range(68)] + [""]
        else:
            faces = topology(points, 120, 80, lambda i: synthetic_texture(0), groups=1)
            textures = [synthetic_texture(i) for i in range(4)]
        sel = []
        for slot in range(30):
            name = selections[slot]
            # A proxy selection is exactly three points: ModelComputation reads the
            # three as apex/direction/up, and anything else is not a frame.
            if name.startswith("proxy:"):
                width = 3
            elif index == 0 and slot == 0:
                width = 276
            elif index == 1 and slot == 0:
                width = 92
            else:
                width = 3 * (slot + 1)
            picked = {(slot * 7 + k) % count for k in range(min(width, count))}
            sel.append((name, picked, {slot % max(1, len(faces))}))
        if index == 1:
            sel[-1] = ("clan_sign", sel[-1][1], sel[-1][2])
        lods.append(Lod(points, normals, uvs, faces, textures,
                        selections=sel,
                        properties=[("lodnoshadow", "1")],
                        proxies=[(proxy_names[i], (i * 0.25, 0.5, -i * 0.5), 1, proxy_order[i])
                                 for i in range(15)],
                        resolution=0.75 if index == 0 else 1.0 + index,
                        mlod_edges=list(range(1679)),
                        vertex_edges=[(i * 3) % count for i in range(count)]))

    # LOD 18: the geometry LOD. 44 components, one empty texture, one material.
    points, normals, uvs = spiral(394, 9.0)
    faces = topology(points, 0, 321, lambda i: "", groups=1)
    lods.append(Lod(points, normals, uvs, faces, [""],
                    selections=[(f"component{i + 1:02d}", {(i * 3 + k) % 394 for k in range(3)},
                                 {i % 321}) for i in range(44)],
                    resolution=1.0e13,
                    mlod_edges=list(range(300)),
                    vertex_edges=[(i * 5) % 394 for i in range(394)]))
    while len(lods) < 19:  # LODs 13..17 pad out to the documented 19
        points, normals, uvs = spiral(300, 9.0)
        lods.insert(13, Lod(points, normals, uvs,
                            topology(points, 80, 60, lambda i: "", groups=1),
                            [""], resolution=14.0,
                            mlod_edges=list(range(200)),
                            vertex_edges=[(i * 2) % 300 for i in range(300)]))
    return lods[:19]


def proxy_structure_lods() -> list[Lod]:
    lods = []
    for index in range(11):
        count = {0: 6037, 5: 127, 10: 735}.get(index, 400)
        face_count = {0: 2475, 5: 38, 10: 577}.get(index, 120)
        # Tall and narrow: 8 m across, 26 m up. LOD 0 is the shadow probe's caster.
        points, normals, uvs = column(count, 4.0, 26.0)
        texture_count = {0: 33, 5: 8, 10: 1}.get(index, 2)
        textures = [synthetic_texture(i) for i in range(texture_count)]
        if index == 10:
            textures = [""]
        groups = {0: 46, 5: 13, 10: 1}.get(index, 1)
        faces = topology(points, 0, face_count, lambda i, t=textures: t[0], groups=groups)
        if index == 0:
            sel = [(f"part_{i + 1:02d}", {(i * 11 + k) % count for k in range(20)}, {i % face_count})
                   for i in range(27)]
            proxies = [(f"proxy_{i:02d}", (i * 0.5, 0.0, 0.0), 1, i) for i in range(26)]
        elif index == 10:
            sel = [(f"component{i + 1:02d}", {(i * 3 + k) % count for k in range(3)}, {i % face_count})
                   for i in range(89)]
            proxies = []
        else:
            sel, proxies = [], []
        lods.append(Lod(points, normals, uvs, faces, textures,
                        selections=sel,
                        properties=[("lodnoshadow", "1")] if index == 0 else [],
                        proxies=proxies,
                        resolution=1.0 + index))
    return lods


def sky_plane_lod() -> Lod:
    points, normals, uvs = spiral(53, 9.9)
    faces = topology(points, 77, 0, lambda i: synthetic_texture(0), groups=1)
    return Lod(points, normals, uvs, faces, [synthetic_texture(0)],
               resolution=1.0, or_hints=16384, and_hints=16384,
               colors=(0xFFB0C0C0, 0xFFB0C0C0, 0),
               mlod_edges=list(range(45)),
               vertex_edges=list(range(53)))


def flat_quad_lod() -> Lod:
    points = [(-1.5625, 0.0, -1.5625), (1.5625, 0.0, -1.5625),
              (1.5625, 0.0, 1.5625), (-1.5625, 0.0, 1.5625)]
    normals = [(0.0, 1.0, 0.0)] * 4
    uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
    faces = [([0, 1, 2, 3], synthetic_texture(0), 0)]
    return Lod(points, normals, uvs, faces, [synthetic_texture(0)],
               resolution=1.0, colors=(0, 0, 0),
               mlod_edges=[0, 1, 2, 3], vertex_edges=[0, 1, 2, 3])


def empty_lod() -> Lod:
    return Lod([], [], [], [], [], resolution=1.0, colors=(0, 0, 0),
               and_hints=0x10000080)


def simple_proxy_lod() -> Lod:
    points, normals, uvs = spiral(8, 1.0)
    faces = topology(points, 8, 0, lambda i: synthetic_texture(0), groups=1)
    return Lod(points, normals, uvs, faces, [synthetic_texture(0)],
               selections=[("proxy:wheel.01", {0, 1, 2}, {0}),
                           ("wheel", {0, 1, 2}, {0})],
               proxies=[("wheel", (1.0, 0.0, 0.0), 1, 0)],
               resolution=1.0, mlod_edges=list(range(8)), vertex_edges=list(range(8)))


def crew_proxy_lod() -> Lod:
    points, normals, uvs = spiral(24, 1.5)
    faces = topology(points, 10, 4, lambda i: synthetic_texture(0), groups=1)
    return Lod(points, normals, uvs, faces, [synthetic_texture(0)],
               selections=[("proxy:crew.01", {0, 1, 2}, {0}),
                           ("proxy:crew.02", {3, 4, 5}, {1})],
               proxies=[("crew", (0.0, 1.0, 0.0), 1, 0),
                        ("crew", (0.0, -1.0, 0.0), 1, 1)],
               resolution=1.0, mlod_edges=list(range(20)), vertex_edges=list(range(24)))


def tree_lod() -> Lod:
    """A four-point SP3X model spanning x [-1, 1], y [-1, 1], z [0, 0].

    `ReadModelProxyBounds` derives radius sqrt(2) and horizontal extent 2 from
    exactly this, and the shadow probe needs faces to raster.
    """
    points = [(-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)]
    normals = [(0.0, 0.0, -1.0)] * 4
    uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
    faces = [([0, 1, 2], synthetic_texture(0), 0), ([0, 2, 3], synthetic_texture(0), 0)]
    return Lod(points, normals, uvs, faces, [synthetic_texture(0)],
               properties=[("lodnoshadow", "1")], resolution=1.0,
               mlod_edges=[0, 1, 2, 3], vertex_edges=[0, 1, 2, 3])


def multi_lod_vehicle_lods() -> list[Lod]:
    """Three SP3X LODs; the first spans x [-1, 1], y [-1, 1], z [-1, 1.8].

    horizontalExtent is max(2, 2.8) = 2.8 and the origin-relative radius is 1.8,
    which is what the far-field probe pins: the z axis is the wide one, and a
    reader that took x alone would under-size the proxy by 40%.
    """
    lods = []
    first = [(-1.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, -1.0, 0.0), (0.0, 1.0, 0.0),
             (0.0, 0.0, -1.0), (0.0, 0.0, 1.8)]
    lods.append(Lod(first, [(0.0, 1.0, 0.0)] * len(first),
                    [(0.0, 0.0)] * len(first),
                    topology(first, 4, 0, lambda i: synthetic_texture(0), groups=1),
                    [synthetic_texture(0)], resolution=1.0))
    for index, count in enumerate([12, 8], start=1):
        points, normals, uvs = spiral(count, 1.0)
        faces = topology(points, 4, 2, lambda i: synthetic_texture(0), groups=1)
        lods.append(Lod(points, normals, uvs, faces, [synthetic_texture(0)],
                        selections=[("proxy:wheel.01", {0, 1, 2}, {0})],
                        proxies=[("wheel", (0.0, 0.0, 0.0), 1, 0)],
                        resolution=1.0 + index))
    return lods


def animated_actor_lods() -> list[Lod]:
    """SP3X with named selections carrying non-zero source weights.

    The weight byte is what AST-018 keeps: a loader that read it only as
    membership would hand every soft selection over hard, and a dropped byte
    reads as zero -- i.e. "not selected" -- so a non-zero weight here is proof
    the byte survived.
    """
    base, normals, uvs = spiral(9, 1.0)
    faces = topology(base, 12, 0, lambda i: "", groups=1)
    return [Lod(base, normals, uvs, faces, [""],
                selections=[("bone_head", {0, 1, 2}, {0}),
                            ("bone_arm", {3, 4, 5}, {1})],
                resolution=1.0,
                mlod_edges=list(range(8)), vertex_edges=list(range(9)))]


def marker_lods() -> list[Lod]:
    """A marker with a frame per orientation: three SP3X LODs, two points each."""
    lods = []
    for index in range(3):
        points = [(0.0, 0.0, float(index)), (0.25, 0.25, float(index))]
        faces = [([0, 1, 0], synthetic_texture(0), 0)]
        lods.append(Lod(points, [(0.0, 1.0, 0.0)] * 2, [(0.0, 0.0)] * 2, faces,
                        [synthetic_texture(0)], resolution=1.0 + index))
    return lods


def empty_mlod_lod() -> Lod:
    """An MLOD whose first LOD has no vertices -- the far-field refusal case."""
    return Lod([], [], [], [], [], resolution=1.0)


def proxy_probe_lod() -> Lod:
    """One proxy marker triangle: the only shape an MLOD proxy can be.

    ModelComputation turns a `proxy:` selection into a frame only when it resolves
    to exactly three vertices, and a point shared by several faces expands to one
    vertex per corner. So the marker's three points are used by one triangle and
    nothing else -- which is what a real proxy marker is.
    """
    # Six points, two disjoint triangles: the marker's three points must be used
    # by no other face, because SP3X expands a point to one vertex per corner.
    points = [(0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0),
              (1.0, 0.0, 0.5), (1.5, 0.0, 0.5), (1.0, 0.0, 1.5)]
    normals = [(1.0, 0.0, 0.0)] * 6
    uvs = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5), (0.5, 1.0)]
    faces = [([0, 1, 2], synthetic_texture(0), 0), ([3, 4, 5], synthetic_texture(0), 0)]
    return Lod(points, normals, uvs, faces, [synthetic_texture(0)],
               selections=[("proxy:wheel.01", {0, 1, 2}, {0}),
                           ("wheel", {0, 1, 2}, {0})],
               resolution=1.0)


def light_disc_lod() -> Lod:
    """A billboard quad: 627.45 m half-size, flat in Z, opaque-black icon."""
    half = 627.450989
    # First and last vertices are on the two -x/+x corners, which is the order the
    # billboard case samples.
    points = [(-half, -half, 0.0), (-half, half, 0.0), (half, half, 0.0), (half, -half, 0.0)]
    uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
    faces = [([0, 1, 2, 3], synthetic_texture(0), 0)]
    return Lod(points, [(0.0, 0.0, 1.0)] * 4, uvs, faces, [synthetic_texture(0)],
               resolution=1.0, colors=(0xFF000000, 0, 0),
               mlod_edges=[0, 1, 2, 3], vertex_edges=[0, 1, 2, 3])


def camera_lod(offset: float) -> Lod:
    """A camera-path helper: two points, no normals, no faces."""
    points = [(0.0, offset, 0.0), (10.0, offset, 0.0)]
    return Lod(points, [], [(0.0, 0.0), (1.0, 0.0)], [], [],
               resolution=1.0, mlod_edges=[], vertex_edges=[])


def main() -> None:
    MLOD_DIR.mkdir(parents=True, exist_ok=True)

    # ODOL v7 corpus: the container the phase-by-phase readers and ODOLLoader
    # cases parse.
    write_odol(HERE / "empty_shape.p3d", [empty_lod()], {})
    write_odol(HERE / "sky_plane.p3d", [sky_plane_lod()], {})
    write_odol(HERE / "flat_quad.p3d", [flat_quad_lod()], {})
    write_odol(HERE / "light_disc.p3d", [light_disc_lod()], {})
    write_odol(HERE / "simple_proxy.p3d", [simple_proxy_lod()], {})
    write_odol(HERE / "crew_proxy.p3d", [crew_proxy_lod()], {})
    write_odol(HERE / "proxy_structure.p3d", proxy_structure_lods(), {})
    vehicle = vehicle_lods()
    write_odol(HERE / "complex_vehicle.p3d", vehicle, {"mass": [1.0] * 16, "mass_total": 16.0})

    # MLOD/SP3X corpus: the editable container, which the far-field bounds probe
    # and the MLOD phase cases read. Two of them live under p3d/ because that is
    # where their tests look for them.
    write_mlod(HERE / "simple_tree.p3d", [tree_lod()])
    write_mlod(HERE / "multi_lod_vehicle.p3d", multi_lod_vehicle_lods())
    write_mlod(HERE / "animated_actor.p3d", animated_actor_lods())
    write_mlod(MLOD_DIR / "complex_vehicle_mlod.p3d", vehicle)
    write_mlod(MLOD_DIR / "animated_marker_mlod.p3d", marker_lods())
    write_mlod(MLOD_DIR / "empty_mlod.p3d", [empty_mlod_lod()])
    write_mlod(MLOD_DIR / "proxy_probe.p3d", [proxy_probe_lod()])
    for name, offset in (("camera_path_a.p3d", 0.0), ("camera_path_b.p3d", 1.0), ("camera_path_c.p3d", 2.0)):
        write_mlod(MLOD_DIR / name, [camera_lod(offset)])

    for path in sorted(list(HERE.glob("*.p3d")) + list(MLOD_DIR.glob("*.p3d"))):
        print(f"wrote {path.relative_to(HERE.parent)}: {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
