// test_p3d_complex_vehicle.cpp - Incremental tests for complex_vehicle.p3d
// Phase-by-phase parsing validation (complex multi-LOD model)

#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <Poseidon/Asset/Formats/P3D/P3DStructures.hpp>
#include <Poseidon/Asset/Formats/BISBinaryStream.hpp>
#include <Poseidon/IO/Streams/QStream.hpp>
#include "../../../test_fixtures.hpp"
#include <fstream>
#include <vector>
#include "../../../test_fixtures.hpp"
#include <stdint.h>
#include <cmath>
#include <cstring>
#include <stdexcept>
#include <string>

static std::vector<char> loadFile(const std::string& path)
{
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file)
    {
        throw std::runtime_error("Failed to open file: " + path);
    }
    auto size = file.tellg();
    file.seekg(0, std::ios::beg);
    std::vector<char> buffer(static_cast<size_t>(size));
    file.read(buffer.data(), size);
    return buffer;
}

TEST_CASE("complex_vehicle.p3d: Phase 1 - Header", "[p3d][complex_vehicle][phase1]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);

    REQUIRE(std::memcmp(header.signature, "ODOL", 4) == 0);
    REQUIRE(header.version == 7);
    REQUIRE(header.lodCount == 19); // 19 LODs
    REQUIRE(header.isValid());
}

TEST_CASE("complex_vehicle.p3d: Phase 2 - Vertex Table LOD 0", "[p3d][complex_vehicle][phase2]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read header
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);

    // Read vertex table for LOD 0 (highest detail)
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);

    // Hardcoded vertex counts
    REQUIRE(vtable.clipFlags.size() == 2617);
    REQUIRE(vtable.uvCoords.size() == 2617);
    REQUIRE(vtable.points.size() == 2617);
    REQUIRE(vtable.normals.size() == 2617);

    // Every vertex must be a finite point inside the authored 9 m extent. The
    // sampled literals this replaces came from the removed model's geometry; the
    // property that matters is that the whole block was walked and read as floats.
    for (const auto& point : vtable.points)
    {
        REQUIRE(std::isfinite(point.x));
        REQUIRE(std::isfinite(point.y));
        REQUIRE(std::isfinite(point.z));
        REQUIRE(std::abs(point.x) <= 9.0001f);
        REQUIRE(std::abs(point.y) <= 9.0001f);
        REQUIRE(std::abs(point.z) <= 9.0001f);
    }
    REQUIRE(vtable.points[0].x != vtable.points[100].x);
    REQUIRE(vtable.points[100].x != vtable.points.back().x);

    // UVs are authored into [0, 1].
    for (const auto& uv : vtable.uvCoords)
    {
        REQUIRE(uv.u >= 0.0f);
        REQUIRE(uv.u <= 1.0f);
        REQUIRE(uv.v >= 0.0f);
        REQUIRE(uv.v <= 1.0f);
    }
}

TEST_CASE("complex_vehicle.p3d: Phase 3 - Bounding Volumes & Textures LOD 0", "[p3d][complex_vehicle][phase3]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read header
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);

    // Read vertex table for LOD 0
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);

    // Read LOD bounds
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);

    // The declared box must enclose the vertex table it belongs to.
    for (const auto& point : vtable.points)
    {
        REQUIRE(point.x >= bounds.minPos.x);
        REQUIRE(point.y >= bounds.minPos.y);
        REQUIRE(point.z >= bounds.minPos.z);
        REQUIRE(point.x <= bounds.maxPos.x);
        REQUIRE(point.y <= bounds.maxPos.y);
        REQUIRE(point.z <= bounds.maxPos.z);
    }
    REQUIRE(bounds.bRadius > 0.0f);
    REQUIRE(bounds.bCenter.x >= bounds.minPos.x);
    REQUIRE(bounds.bCenter.x <= bounds.maxPos.x);

    // Read textures
    auto tex = Poseidon::Asset::Formats::P3D::readTextures(reader);

    // 69 slots: 68 authored surfaces and one empty name, which is legal and means
    // "the untextured surface".
    REQUIRE(tex.texturePaths.size() == 69);
    REQUIRE(tex.texturePaths[0] == "data\\synthetic\\surface_00.pac");
    REQUIRE(tex.texturePaths[10] == "data\\synthetic\\surface_10.pac");
    REQUIRE(tex.texturePaths.back() == "");
}

TEST_CASE("complex_vehicle.p3d: Phase 4 - LOD Edges, Faces, Sections LOD 0", "[p3d][complex_vehicle][phase4]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 4
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);

    // Phase 4: Edges
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    REQUIRE(edges.mlodIndex.edges.size() == 1679);
    REQUIRE(edges.vertexIndex.edges.size() == 2617);

    // Both edge arrays are the authored sequences: the mlod index runs 0..1678 and
    // the vertex index is (i * 3) mod 2617, so these are exact, not sampled.
    REQUIRE(edges.mlodIndex.edges[0] == 0);
    REQUIRE(edges.mlodIndex.edges[100] == 100);
    REQUIRE(edges.mlodIndex.edges.back() == 1678);

    REQUIRE(edges.vertexIndex.edges[0] == 0);
    REQUIRE(edges.vertexIndex.edges[100] == 300);
    REQUIRE(edges.vertexIndex.edges.back() == 2614);

    // Phase 4: Faces
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    REQUIRE(faces.count == 1341);
    REQUIRE(faces.offsetToSections > 0);
    REQUIRE(faces.faces.size() == 1341);

    // Every face is a triangle or a quad over the vertex block, in the authored
    // texture slot 0, with no face flags. The counts below are the split the
    // generator wrote: 549 triangles then 792 quads.
    size_t triangles = 0, quads = 0;
    for (const auto& face : faces.faces)
    {
        REQUIRE(face.flags == 0);
        REQUIRE(face.textureIndex == 0);
        REQUIRE((face.vertexCount == 3 || face.vertexCount == 4));
        REQUIRE(face.vertexIndices.size() == face.vertexCount);
        for (uint16_t index : face.vertexIndices)
            REQUIRE(index < vtable.points.size());
        ++(face.vertexCount == 3 ? triangles : quads);
    }
    REQUIRE(triangles == 549);
    REQUIRE(quads == 792);

    const auto& f0 = faces.faces[0];
    REQUIRE(f0.vertexCount == 3);
    REQUIRE(f0.vertexIndices.size() == 3);

    // Validate texture indices in range
    for (const auto& face : faces.faces)
    {
        if (face.textureIndex >= 0)
        {
            REQUIRE(face.textureIndex < static_cast<int16_t>(textures.texturePaths.size()));
        }
    }

    // Phase 4: Sections
    auto sections = Poseidon::Asset::Formats::P3D::readSections(reader);
    REQUIRE(sections.sections.size() == 101);

    // Sections are byte ranges over the face block, one per authored material
    // run: the first starts at 0, the last ends where the face block does, and the
    // whole sequence is ordered and gapless.
    REQUIRE(sections.sections.size() == 101);
    REQUIRE(sections.sections[0].faceIndexLowerBound == 0);
    REQUIRE(sections.sections[0].material == 0);
    REQUIRE(sections.sections[0].textureIndex == 0);
    REQUIRE(sections.sections[0].special == 0);
    REQUIRE(sections.sections.back().faceIndexUpperBound == faces.offsetToSections);
    for (size_t i = 1; i < sections.sections.size(); ++i)
    {
        REQUIRE(sections.sections[i].faceIndexLowerBound == sections.sections[i - 1].faceIndexUpperBound);
        REQUIRE(sections.sections[i].faceIndexUpperBound > sections.sections[i].faceIndexLowerBound);
        REQUIRE(sections.sections[i].material == static_cast<int32_t>(i));
    }

    // Validate section texture indices
    for (const auto& section : sections.sections)
    {
        if (section.textureIndex >= 0)
        {
            REQUIRE(section.textureIndex < static_cast<int16_t>(textures.texturePaths.size()));
        }
    }
}

TEST_CASE("complex_vehicle.p3d: Phase 5 - Named Selections LOD 0", "[p3d][complex_vehicle][phase5]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 5
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections = Poseidon::Asset::Formats::P3D::readSections(reader);

    // Phase 5: Named Selections
    auto namedSelections = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);

    REQUIRE(namedSelections.selections.size() == 30);

    // Sample named selections
    const auto& sel0 = namedSelections.selections[0];
    REQUIRE(sel0.name == "part_01");
    REQUIRE(sel0.vertexIndices.size() == 276);
    for (uint16_t index : sel0.vertexIndices)
        REQUIRE(index < vtable.points.size());

    const auto& sel1 = namedSelections.selections[1];
    REQUIRE(sel1.name == "part_02");
    REQUIRE_FALSE(sel1.vertexIndices.empty());

    const auto& sel10 = namedSelections.selections[10];
    REQUIRE(sel10.name == "proxy:cargo.01");
    // A proxy selection is exactly three vertices: apex, direction, up.
    REQUIRE(sel10.vertexIndices.size() == 3);

    const auto& selLast = namedSelections.selections.back();
    REQUIRE(selLast.name == "proxy:flag_alone.01");
    REQUIRE(selLast.vertexIndices.size() == 3);

    // Validate vertex indices are in range
    for (const auto& selection : namedSelections.selections)
    {
        for (uint32_t idx : selection.vertexIndices)
        {
            REQUIRE(idx < vtable.points.size());
        }
    }
}

TEST_CASE("complex_vehicle.p3d: Phase 6 - Named Properties LOD 0", "[p3d][complex_vehicle][phase6]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 6
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections = Poseidon::Asset::Formats::P3D::readSections(reader);
    auto namedSelections = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);

    // Phase 6: Named Properties
    auto namedProperties = Poseidon::Asset::Formats::P3D::readNamedProperties(reader);

    REQUIRE(namedProperties.properties.size() == 1);

    const auto& prop0 = namedProperties.properties[0];
    REQUIRE(prop0.property == "lodnoshadow"); // LOD doesn't cast shadow
    REQUIRE(prop0.value == "1");
}

TEST_CASE("complex_vehicle.p3d: Phase 7 - Frames LOD 0", "[p3d][complex_vehicle][phase7]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 7
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections = Poseidon::Asset::Formats::P3D::readSections(reader);
    auto namedSelections = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);
    auto namedProperties = Poseidon::Asset::Formats::P3D::readNamedProperties(reader);

    // Phase 7: Frames
    auto frames = Poseidon::Asset::Formats::P3D::readFrames(reader);

    // complex_vehicle LOD 0 has no animation frames
    REQUIRE(frames.frames.size() == 0);
}

TEST_CASE("complex_vehicle.p3d: Phase 8 - LOD End Data LOD 0", "[p3d][complex_vehicle][phase8]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 8
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections = Poseidon::Asset::Formats::P3D::readSections(reader);
    auto namedSelections = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);
    auto namedProperties = Poseidon::Asset::Formats::P3D::readNamedProperties(reader);
    auto frames = Poseidon::Asset::Formats::P3D::readFrames(reader);

    // Phase 8: LOD End Data
    auto lodEndData = Poseidon::Asset::Formats::P3D::readLODEndData(reader);

    // Hardcoded color values (RGBA format)
    REQUIRE(lodEndData.colorTop == 0xFF808080u);
    REQUIRE(lodEndData.color == 0xFF808080u);
    REQUIRE(lodEndData.special == 0);
}

TEST_CASE("complex_vehicle.p3d: Phase 9 - Proxies LOD 0", "[p3d][complex_vehicle][phase9]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 9 (complete LOD)
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections = Poseidon::Asset::Formats::P3D::readSections(reader);
    auto namedSelections = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);
    auto namedProperties = Poseidon::Asset::Formats::P3D::readNamedProperties(reader);
    auto frames = Poseidon::Asset::Formats::P3D::readFrames(reader);
    auto lodEndData = Poseidon::Asset::Formats::P3D::readLODEndData(reader);

    // Phase 9: Proxies
    auto proxies = Poseidon::Asset::Formats::P3D::readProxies(reader);

    REQUIRE(proxies.proxies.size() == 15);

    // Sample proxies
    const auto& proxy0 = proxies.proxies[0];
    REQUIRE(proxy0.name == "crew"); // Short name: prefix and id stripped
    REQUIRE(proxy0.sequenceId == 1);
    REQUIRE(proxy0.namedSelectionIndex == 9); // namedSelections[9] is "proxy:crew.01"

    // Transform matrix (4x3) - identity rotation with translation
    REQUIRE(proxy0.transform[0].x == Catch::Approx(1.0f));
    REQUIRE(proxy0.transform[0].y == Catch::Approx(0.0f).margin(0.00001f));
    REQUIRE(proxy0.transform[0].z == Catch::Approx(0.0f).margin(0.00001f));
    REQUIRE(proxy0.transform[1].x == Catch::Approx(0.0f).margin(0.00001f));
    REQUIRE(proxy0.transform[1].y == Catch::Approx(1.0f));
    REQUIRE(proxy0.transform[1].z == Catch::Approx(0.0f).margin(0.00001f));
    REQUIRE(proxy0.transform[2].x == Catch::Approx(0.0f).margin(0.00001f));
    REQUIRE(proxy0.transform[2].y == Catch::Approx(0.0f).margin(0.00001f));
    REQUIRE(proxy0.transform[2].z == Catch::Approx(1.0f));
    // Translation is the authored per-instance offset: row 3 is (i * 0.25, 0.5, -i * 0.5).
    REQUIRE(proxy0.transform[3].x == Catch::Approx(0.0f));
    REQUIRE(proxy0.transform[3].y == Catch::Approx(0.5f));
    REQUIRE(proxy0.transform[3].z == Catch::Approx(0.0f));

    const auto& proxy1 = proxies.proxies[1];
    REQUIRE(proxy1.name == "cargo");           // Short name in file
    REQUIRE(proxy1.namedSelectionIndex == 10); // Full name in namedSelections[10]: "proxy:cargo.01"

    const auto& proxy5 = proxies.proxies[5];
    REQUIRE(proxy5.name == "cargo");           // Short name in file
    REQUIRE(proxy5.namedSelectionIndex == 14); // Full name in namedSelections[14]: "proxy:cargo.04"

    const auto& proxyLast = proxies.proxies.back();
    REQUIRE(proxyLast.name == "flag_alone");      // Short name in file
    REQUIRE(proxyLast.namedSelectionIndex == 29); // Full name in namedSelections[29]: "proxy:flag_alone.01"

    // Validate named selection indices are in range
    for (const auto& proxy : proxies.proxies)
    {
        REQUIRE(proxy.namedSelectionIndex < namedSelections.selections.size());
    }
}

TEST_CASE("complex_vehicle.p3d: Multi-LOD - Load LOD 0 and LOD 1", "[p3d][complex_vehicle][multilod]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read header
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    REQUIRE(header.lodCount == 19);

    // === LOD 0 (highest detail) ===
    auto vtable0 = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds0 = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures0 = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges0 = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces0 = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections0 = Poseidon::Asset::Formats::P3D::readSections(reader);
    auto namedSelections0 = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);
    auto namedProperties0 = Poseidon::Asset::Formats::P3D::readNamedProperties(reader);
    auto frames0 = Poseidon::Asset::Formats::P3D::readFrames(reader);
    auto lodEndData0 = Poseidon::Asset::Formats::P3D::readLODEndData(reader);
    auto proxies0 = Poseidon::Asset::Formats::P3D::readProxies(reader);

    // Basic assertions for LOD 0
    REQUIRE(vtable0.points.size() == 2617);
    REQUIRE(textures0.texturePaths.size() == 69);
    REQUIRE(textures0.texturePaths[0] == "data\\synthetic\\surface_00.pac");
    REQUIRE(faces0.count == 1341);
    REQUIRE(sections0.sections.size() == 101);
    REQUIRE(namedSelections0.selections.size() == 30);
    REQUIRE(namedSelections0.selections[0].name == "part_01");
    REQUIRE(namedSelections0.selections.back().name == "proxy:flag_alone.01");
    REQUIRE(namedProperties0.properties.size() == 1);
    REQUIRE(namedProperties0.properties[0].property == "lodnoshadow");
    REQUIRE(namedProperties0.properties[0].value == "1");
    REQUIRE(frames0.frames.size() == 0);
    REQUIRE(proxies0.proxies.size() == 15);
    REQUIRE(proxies0.proxies[0].name == "crew");
    REQUIRE(proxies0.proxies[0].namedSelectionIndex == 9);
    REQUIRE(proxies0.proxies.back().name == "flag_alone");
    REQUIRE(proxies0.proxies.back().namedSelectionIndex == 29);

    // === LOD 1 (second level of detail) ===
    auto vtable1 = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds1 = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures1 = Poseidon::Asset::Formats::P3D::readTextures(reader);
    auto edges1 = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    auto faces1 = Poseidon::Asset::Formats::P3D::readFaces(reader);
    auto sections1 = Poseidon::Asset::Formats::P3D::readSections(reader);
    auto namedSelections1 = Poseidon::Asset::Formats::P3D::readNamedSelections(reader);
    auto namedProperties1 = Poseidon::Asset::Formats::P3D::readNamedProperties(reader);
    auto frames1 = Poseidon::Asset::Formats::P3D::readFrames(reader);
    auto lodEndData1 = Poseidon::Asset::Formats::P3D::readLODEndData(reader);
    auto proxies1 = Poseidon::Asset::Formats::P3D::readProxies(reader);

    // Basic assertions for LOD 1 - should have fewer vertices/faces than LOD 0
    REQUIRE(vtable1.points.size() == 1905); // LOD 1 has less detail than LOD 0 (2617)
    REQUIRE(vtable1.points.size() < vtable0.points.size());

    REQUIRE(textures1.texturePaths.size() > 0);
    REQUIRE(!textures1.texturePaths[0].empty()); // First texture should exist

    REQUIRE(faces1.count > 0);
    REQUIRE(faces1.count < faces0.count); // LOD 1 has fewer faces

    REQUIRE(sections1.sections.size() > 0);

    REQUIRE(namedSelections1.selections.size() == 30); // Same as LOD 0
    REQUIRE(!namedSelections1.selections[0].name.empty());
    REQUIRE(namedSelections1.selections.back().name == "clan_sign"); // Last selection in LOD 1

    REQUIRE(namedProperties1.properties.size() == 1);
    REQUIRE(namedProperties1.properties[0].property == "lodnoshadow");
    REQUIRE(namedProperties1.properties[0].value == "1");

    REQUIRE(frames1.frames.size() == 0);

    REQUIRE(proxies1.proxies.size() == 15); // Same proxy count as LOD 0
    // Both LODs reuse one role table, so the names and their selection indices
    // must agree with LOD 0's rather than merely being plausible.
    for (size_t i = 0; i < proxies1.proxies.size(); ++i)
    {
        REQUIRE(proxies1.proxies[i].name == proxies0.proxies[i].name);
        REQUIRE(proxies1.proxies[i].namedSelectionIndex == proxies0.proxies[i].namedSelectionIndex);
    }
}
TEST_CASE("complex_vehicle.p3d: All 19 LODs - Parse through to last LOD", "[p3d][complex_vehicle][all-lods]")
{
    auto data = loadFile(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    REQUIRE(header.lodCount == 19);

    // Read LODs 0-17 quickly with basic sanity checks
    for (int i = 0; i < header.lodCount - 1; ++i)
    {
        auto lod = Poseidon::Asset::Formats::P3D::readCompleteLOD(reader);
        REQUIRE(lod.vertexTable.points.size() > 0);
    }

    // Read and validate last LOD (LOD 18) in detail
    auto lastLOD = Poseidon::Asset::Formats::P3D::readCompleteLOD(reader);

    // Vertex table
    REQUIRE(lastLOD.vertexTable.points.size() == 394);
    REQUIRE(lastLOD.vertexTable.normals.size() == 394);
    REQUIRE(lastLOD.vertexTable.uvCoords.size() == 394);

    // Textures
    REQUIRE(lastLOD.textures.texturePaths.size() == 1);
    REQUIRE(lastLOD.textures.texturePaths[0] == "");

    // Faces and sections
    REQUIRE(lastLOD.faces.count == 321);
    REQUIRE(lastLOD.faces.faces.size() == 321);
    REQUIRE(lastLOD.sections.sections.size() == 1);

    // Named selections - 44 component selections
    REQUIRE(lastLOD.namedSelections.selections.size() == 44);
    REQUIRE(lastLOD.namedSelections.selections[0].name == "component01");
    REQUIRE(lastLOD.namedSelections.selections[1].name == "component02");
    REQUIRE(lastLOD.namedSelections.selections[2].name == "component03");
    REQUIRE(lastLOD.namedSelections.selections[3].name == "component04");
    REQUIRE(lastLOD.namedSelections.selections[4].name == "component05");
    REQUIRE(lastLOD.namedSelections.selections[43].name == "component44");

    // Properties, frames, proxies
    REQUIRE(lastLOD.namedProperties.properties.size() == 0);
    REQUIRE(lastLOD.frames.frames.size() == 0);
    REQUIRE(lastLOD.proxies.proxies.size() == 0);
}
