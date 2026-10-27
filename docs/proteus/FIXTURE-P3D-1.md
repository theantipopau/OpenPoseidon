# FIXTURE-P3D-1 — Synthetic P3D/MLOD corpus (done-unverified)

Status: done-unverified (unit tests only; nothing GPU-verified)

## What was wrong

`FIXTURE-SYNTH-1` closed the texture and shape-fuzz gaps and left one follow-up open:

> `p3d/` render/loader corpus (`simple_proxy`, `complex_vehicle`, `flat_quad`, …):
> needs ODOL/MLOD files with proxies, weighted selections, multi-LOD — a
> model-generator ticket with format research, not a bytes ticket.

The corpus the P3D phase readers, the loader/parity cases, the model-DDC digest pin,
the C API cases and the shadow/asset probes were written against was licensed Bohemia
data that the source release omits. Twelve cases across `[p3d]` / `[mlod]` therefore
reported SKIP (`GET_FIXTURE` → `FIXTURE-SKIP-1`), and the ones that did run only ran
because they asserted structure, not content.

## What changed

- `tests/fixtures/p3d/make_p3d_corpus.py` (new): one model description compiled into
  **both** wire encodings, so `p3d/complex_vehicle.p3d` (ODOL 7) and
  `mlod/complex_vehicle_mlod.p3d` (MLOD 1.1 + SP3X) are the same model — which is what
  the MLOD/ODOL parity cases exist to check. Wire layouts follow
  `engine/Poseidon/Asset/Formats/P3D/P3DStructures.hpp` (ODOL v7) and
  `.../P3D/MLODStructures.hpp` (MLOD 1.1 + SP3X + TAGG). Arrays at or above
  `COMPRESSION_THRESHOLD` (1024 bytes, `BISBinaryStream.hpp`) are emitted as
  LZSS streams made of literals plus the trailing checksum, which
  `SsCompress::Decode` accepts as legal input — no match finder needed.
- Both `p3d/` and `mlod/` fixtures are now generated data, not samples:

  | file | encoding | role |
  | --- | --- | --- |
  | `p3d/complex_vehicle.p3d` | ODOL 7 | 19 LODs, 15 proxies, 30+14 selections, 68 textures + sections |
  | `mlod/complex_vehicle_mlod.p3d` | SP3X | the same model, editable form |
  | `p3d/proxy_structure.p3d` | ODOL 7 | 11 LODs, 26 proxies, 27 selections, 46 sections at LOD 0 |
  | `p3d/crew_proxy.p3d`, `p3d/simple_proxy.p3d` | ODOL 7 | single-LOD proxy carriers |
  | `p3d/sky_plane.p3d`, `p3d/light_disc.p3d`, `p3d/flat_quad.p3d` | ODOL 7 | hint/colour/billboard cases (627.45 m quad, winding, `0x4000` hints) |
  | `p3d/empty_shape.p3d`, `mlod/empty_mlod.p3d` | ODOL 7 / SP3X | the zero-vertex refusals |
  | `p3d/simple_tree.p3d`, `p3d/multi_lod_vehicle.p3d`, `p3d/animated_actor.p3d` | SP3X | far-field bounds, wide-axis proxy, weighted selections |
  | `mlod/animated_marker_mlod.p3d`, `mlod/proxy_probe.p3d`, `mlod/camera_path_{a,b,c}.p3d` | SP3X | marker frames, proxy extraction, 2-point helpers |

- Tests that asserted **original content** now assert the synthetic content:
  `test_p3d_complex_vehicle` selection names, `test_p3d_proxy_structure` selection
  names, `test_poseidon_formats_capi` model format (`mlod/complex_vehicle_mlod.p3d`
  is the MLOD build; `p3d/complex_vehicle.p3d` is the ODOL 7 one), and the
  `test_ast004_model_ddc` base-form digest pin (the case documents re-pinning by
  running it and reading the expansion).
- Counts the cases document as *invariants* are reproduced, because those numbers are
  the contract: 19 LODs, 6037-point/2475-face proxy LOD with 26 proxies, 44 geometry
  components, 89-component roadway LOD, 627.45 m billboard half-size.

Two shapes encode a property the tests measure rather than a look:

- `p3d/proxy_structure.p3d` is a **tall** structure (8 m across, 26 m up). `ShadowInspect`
  asserts an overhead sun casts a *smaller* shadow fraction than a 45° one; a sphere
  casts the same silhouette at every elevation, so it cannot show the light direction
  reaching the shadow matrices at all.
- `mlod/proxy_probe.p3d` is **two disjoint triangles**. SP3X expands a point to one
  vertex per face corner, so the marker's three points must be used by no other face or
  the `proxy:` selection resolves to more than three vertices and `generateProxiesFromSelections`
  legitimately skips it.

## Verification

| run | before | after |
| --- | --- | --- |
| `PoseidonTests.exe` | 19 `p3d/`+`mlod/` fixtures absent (of 34) | 0 `p3d/`/`mlod/` fixtures absent (19 left, all other families) |
| `[p3d],[mlod]` | 12 cases skipped on absent fixtures | 177 cases: 168 passed, 0 failed, 9 skipped; 344,603 assertions, 0 failed |
| `[ShadowInspect]` | 1 failed (elevation inversion on a sphere) | 5 cases / 23 assertions pass |
| `PoseidonTests.exe` full | 3 failed, 154 skipped | 3 failed, 154 skipped (the three documented pre-existing failures: dev-panel `ImGui::` bypasses, `DrainInstanceOps` lookup ordering, local DayZ corpus revisions) |
| `PoseidonCoreTests.exe` | 1 failed (qstream mod-path escape) | 1 failed (same), 895 cases: 870 passed, 24 skipped |

Regenerate with `python tests/fixtures/p3d/make_p3d_corpus.py`; it is byte-stable
(no timestamps, no environment input).

## What is still skipped, and why it stays skipped

The 9 remaining `[p3d]`/`[mlod]` skips are the env-var corpus gates, not fixture
absence: `POSEIDON_ODOL73_FIXTURE` (5 in `test_odol_revision`, 1 in
`test_model_proxy_bounds`), `POSEIDON_ODOL73_CORPUS` (2), `POSEIDON_ODOL40_CORPUS` (1).
They survey real extracted revision-40/73 models; a synthetic file cannot stand in for
the property under test (that the reader survives whatever a retail exporter wrote).

## Follow-ups

- `wrp/`, `jpg/`, `pbo/`, `rtm/`, `audio/`, `cfg/`, `png/` families are still absent.
- The corpus is generated, so a format change (ODOL header, SP3X face record) must
  update the generator in the same change; nothing checks the generator against the
  readers except the tests that load the output.
