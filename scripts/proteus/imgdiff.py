#!/usr/bin/env python3
"""T19 perceptual diff: stdlib-only PNG compare (SSIM dissimilarity + color MAE).

Choice: SSIM (Wang et al. 2004) implemented from scratch — no dependency, so no
licence decision and nothing to vendor (see PROVENANCE.md). FLIP was rejected:
it needs a display model + heavier numerics for no benefit at golden-threshold use.

Metric: mean SSIM over 8x8 box windows (integral images, O(N)) on luma downscaled
to <=320px wide, plus color MAE on downscaled RGB. Report both; pass/fail uses both.
dissim = 1 - mean_ssim, so 0.0 == identical.
"""
import math
import os
import struct
import sys
import zlib

_SIG = b"\x89PNG\r\n\x1a\n"
_MAX_W = 320  # compare width cap; box-downscale keeps it O(N) and kills sensor noise


class DiffError(Exception):
    pass


def _chunks(data):
    assert data[:8] == _SIG, "not a PNG"
    pos = 8
    while pos < len(data):
        (ln,) = struct.unpack(">I", data[pos:pos + 4])
        typ = data[pos + 4:pos + 8]
        yield typ, data[pos + 8:pos + 8 + ln]
        pos += 12 + ln


def read_png(path):
    """-> (w, h, bytearray RGB). Only 8-bit non-interlaced RGB/RGBA; else DiffError."""
    with open(path, "rb") as fh:
        data = fh.read()
    w = h = None
    depth = ctype = interlace = None
    raw = bytearray()
    for typ, body in _chunks(data):
        if typ == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
        elif typ == b"IDAT":
            raw += body
    if w is None:
        raise DiffError(f"{path}: no IHDR")
    if depth != 8 or interlace != 0 or ctype not in (2, 6):
        raise DiffError(f"{path}: need 8-bit non-interlaced RGB/RGBA, got depth={depth} type={ctype}")
    ch = 3 if ctype == 2 else 4
    stride = w * ch
    px = zlib.decompress(bytes(raw))
    if len(px) != h * (stride + 1):
        raise DiffError(f"{path}: truncated scanlines")
    out = bytearray(w * h * 3)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = px[p]
        p += 1
        cur = bytearray(px[p:p + stride])
        p += stride
        if f == 1:  # Sub
            for i in range(ch, stride):
                cur[i] = (cur[i] + cur[i - ch]) & 255
        elif f == 2:  # Up
            for i in range(stride):
                cur[i] = (cur[i] + prev[i]) & 255
        elif f == 3:  # Average
            for i in range(stride):
                a = cur[i - ch] if i >= ch else 0
                cur[i] = (cur[i] + ((a + prev[i]) >> 1)) & 255
        elif f == 4:  # Paeth
            for i in range(stride):
                a = cur[i - ch] if i >= ch else 0
                b = prev[i]
                c = prev[i - ch] if i >= ch else 0
                q = a + b - c
                pa, pb, pc = abs(q - a), abs(q - b), abs(q - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                cur[i] = (cur[i] + pr) & 255
        elif f != 0:
            raise DiffError(f"{path}: unknown filter {f}")
        for x in range(w):
            o = (y * w + x) * 3
            s = x * ch
            out[o:o + 3] = cur[s:s + 3]
        prev = cur
    return w, h, out


def write_png(path, w, h, rgb):
    def chunk(typ, body):
        return struct.pack(">I", len(body)) + typ + body + struct.pack(">I", zlib.crc32(typ + body))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    rows = bytearray()
    for y in range(h):
        rows.append(0)
        rows += rgb[y * w * 3:(y + 1) * w * 3]
    with open(path, "wb") as fh:
        fh.write(_SIG + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(rows))) + chunk(b"IEND", b""))


def _luma(rgb):
    return [0.2126 * rgb[i] + 0.7152 * rgb[i + 1] + 0.0722 * rgb[i + 2] for i in range(0, len(rgb), 3)]


def _box_downscale(vals, w, h, is_rgb):
    """Box-average to width<=_MAX_W. Returns (vals, w2, h2)."""
    if w <= _MAX_W:
        return vals, w, h
    s = math.ceil(w / _MAX_W)
    w2, h2 = w // s, h // s
    nch = 3 if is_rgb else 1
    out = [0.0] * (w2 * h2 * nch)
    for y2 in range(h2):
        for x2 in range(w2):
            for c in range(nch):
                t = 0.0
                for dy in range(s):
                    for dx in range(s):
                        t += vals[((y2 * s + dy) * w + x2 * s + dx) * nch + c]
                out[(y2 * w2 + x2) * nch + c] = t / (s * s)
    return out, w2, h2


def _integral(g, w, h):
    I = [0.0] * ((w + 1) * (h + 1))
    for y in range(h):
        rs = 0.0
        for x in range(w):
            rs += g[y * w + x]
            I[(y + 1) * (w + 1) + x + 1] = I[y * (w + 1) + x + 1] + rs
    return I


def _win(I, w, x0, y0, x1, y1):
    return I[y1 * (w + 1) + x1] - I[y0 * (w + 1) + x1] - I[y1 * (w + 1) + x0] + I[y0 * (w + 1) + x0]


def mean_ssim(a, b, w, h, win=8):
    """Mean SSIM over win×win box windows via integral images."""
    C1, C2 = 6.5025, 58.5225  # (0.01*255)^2, (0.03*255)^2
    Ia, Ib = _integral(a, w, h), _integral(b, w, h)
    Ia2 = _integral([v * v for v in a], w, h)
    Ib2 = _integral([v * v for v in b], w, h)
    Iab = _integral([x * y for x, y in zip(a, b)], w, h)
    n = win * win
    tot, cnt = 0.0, 0
    for y0 in range(0, h - win + 1):
        for x0 in range(0, w - win + 1):
            x1, y1 = x0 + win, y0 + win
            mx, my = _win(Ia, w, x0, y0, x1, y1) / n, _win(Ib, w, x0, y0, x1, y1) / n
            sx2 = _win(Ia2, w, x0, y0, x1, y1) / n - mx * mx
            sy2 = _win(Ib2, w, x0, y0, x1, y1) / n - my * my
            sxy = _win(Iab, w, x0, y0, x1, y1) / n - mx * my
            tot += ((2 * mx * my + C1) * (2 * sxy + C2)
                    / ((mx * mx + my * my + C1) * (sx2 + sy2 + C2)))
            cnt += 1
    return tot / cnt


def compare(a_path, b_path):
    wa, ha, pa = read_png(a_path)
    wb, hb, pb = read_png(b_path)
    if (wa, ha) != (wb, hb):
        raise DiffError(f"size mismatch: {wa}x{ha} vs {wb}x{hb}")
    ga, w2, h2 = _box_downscale(_luma(pa), wa, ha, False)
    gb, _, _ = _box_downscale(_luma(pb), wb, hb, False)
    ra, _, _ = _box_downscale([float(v) for v in pa], wa, ha, True)
    rb, _, _ = _box_downscale([float(v) for v in pb], wb, hb, True)
    n = len(ra)
    mae = sum(abs(x - y) for x, y in zip(ra, rb)) / n
    return {"dissim": 1.0 - mean_ssim(ga, gb, w2, h2), "color_mae": mae, "w": wa, "h": ha}


def _solid(w, h, r, g, b):
    px = bytearray()
    for _ in range(w * h):
        px += bytes((r, g, b))
    return px


def selftest(tmp):
    import tempfile
    d = tempfile.mkdtemp(prefix="imgdiff-")
    W, H = 160, 90
    grad = bytearray()
    for y in range(H):
        for x in range(W):
            grad += bytes((x * 255 // W, y * 255 // H, 128))
    p_id1, p_id2 = os.path.join(d, "a.png"), os.path.join(d, "b.png")
    write_png(p_id1, W, H, grad)
    write_png(p_id2, W, H, grad)
    r = compare(p_id1, p_id2)
    assert r["dissim"] == 0.0 and r["color_mae"] == 0.0, f"identical must be 0: {r}"
    # roundtrip preserves pixels exactly
    assert read_png(p_id1)[2] == grad, "write/read roundtrip must be lossless"
    # small global lift: small dissim, mae ~= lift
    p_lift = os.path.join(d, "lift.png")
    write_png(p_lift, W, H, bytearray(
        b for y in range(H) for x in range(W)
        for b in (min(x * 255 // W + 8, 255), min(y * 255 // H + 8, 255), 136)))
    rl = compare(p_id1, p_lift)
    assert 0.0 < rl["dissim"] < 0.05, f"lift dissim small: {rl}"
    assert 7.0 < rl["color_mae"] < 9.0, f"lift mae ~= 8: {rl}"
    # 2px shift of a smooth gradient: structural change is tiny
    p_shift = os.path.join(d, "shift.png")
    sh = bytearray()
    for y in range(H):
        for x in range(W):
            xs = min(x + 2, W - 1)
            sh += bytes((xs * 255 // W, y * 255 // H, 128))
    write_png(p_shift, W, H, sh)
    rs = compare(p_id1, p_shift)
    assert rs["dissim"] < 0.01, f"2px gradient shift must be tiny: {rs}"
    # white noise: structurally destroyed
    import random
    random.seed(7)
    nz = bytearray(random.randrange(256) for _ in range(W * H * 3))
    p_noise = os.path.join(d, "noise.png")
    write_png(p_noise, W, H, nz)
    rn = compare(p_id1, p_noise)
    assert rn["dissim"] > 0.3, f"noise must be large: {rn}"
    assert rn["dissim"] > 10 * rs["dissim"], f"noise >> shift: {rn} vs {rs}"
    # size mismatch errors, doesn't silently rescale
    p_small = os.path.join(d, "small.png")
    write_png(p_small, 80, 45, _solid(80, 45, 0, 0, 0))
    try:
        compare(p_id1, p_small)
        raise AssertionError("size mismatch must raise")
    except DiffError:
        pass
    # RGBA input accepted (alpha ignored)
    p_rgba = os.path.join(d, "rgba.png")
    raw = bytearray()
    for y in range(H):
        raw.append(0)
        for x in range(W):
            raw += bytes((x * 255 // W, y * 255 // H, 128, 200))
    def chunk(typ, body):
        return struct.pack(">I", len(body)) + typ + body + struct.pack(">I", zlib.crc32(typ + body))
    with open(p_rgba, "wb") as fh:
        fh.write(_SIG + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
                 + chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b""))
    rr = compare(p_id1, p_rgba)
    assert rr["dissim"] == 0.0, f"RGBA with same RGB must match: {rr}"
    print(f"imgdiff: selftest OK (identical=0 lift={rl['dissim']:.4f} shift={rs['dissim']:.5f} noise={rn['dissim']:.3f})")


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--selftest":
        import tempfile
        selftest(tempfile.gettempdir())
    elif len(sys.argv) == 3:
        try:
            r = compare(sys.argv[1], sys.argv[2])
        except DiffError as e:
            print(f"imgdiff: ERROR {e}")
            sys.exit(2)
        print(f"dissim={r['dissim']:.5f} color_mae={r['color_mae']:.3f} size={r['w']}x{r['h']}")
    else:
        print("usage: imgdiff.py A.png B.png | imgdiff.py --selftest")
        sys.exit(2)
