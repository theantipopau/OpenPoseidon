// test_p3d_sky_plane.cpp - Incremental tests for sky_plane.p3d
// Phase-by-phase parsing validation

#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <Poseidon/Asset/Formats/P3D/P3DStructures.hpp>
#include <Poseidon/Asset/Formats/BISBinaryStream.hpp>
#include <Poseidon/IO/Streams/QStream.hpp>
#include "../../../test_fixtures.hpp"
#include <fstream>
#include <vector>
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

TEST_CASE("sky_plane.p3d: Phase 1 - Header", "[p3d][sky_plane][phase1]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);

    REQUIRE(std::memcmp(header.signature, "ODOL", 4) == 0);
    REQUIRE(header.version == 7);
    REQUIRE(header.lodCount == 1);
    REQUIRE(header.isValid());
}

TEST_CASE("sky_plane.p3d: Phase 2 - Vertex Table", "[p3d][sky_plane][phase2]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read header
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    REQUIRE(header.lodCount == 1);

    // Read vertex table for LOD 0
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);

    // Hardcoded array sizes
    REQUIRE(vtable.clipFlags.size() == 53);
    REQUIRE(vtable.uvCoords.size() == 53);
    REQUIRE(vtable.points.size() == 53);
    REQUIRE(vtable.normals.size() == 53);

    // Every vertex must be a number inside the authored 9.9 m extent. These used
    // to be three sampled coordinates of the removed model; a sample of a fixture
    // that no longer exists only ever tested the old file, while the property that
    // survives a corpus change is "the walk covers the whole block, and reads it
    // as floats rather than as reused bytes".
    for (const auto& point : vtable.points)
    {
        REQUIRE(std::isfinite(point.x));
        REQUIRE(std::isfinite(point.y));
        REQUIRE(std::isfinite(point.z));
        REQUIRE(std::abs(point.x) <= 9.9001f);
        REQUIRE(std::abs(point.y) <= 9.9001f);
        REQUIRE(std::abs(point.z) <= 9.9001f);
    }
    // The spiral is not degenerate: no two samples of the block may be the same
    // point, or the samples above would all be testing one vertex.
    REQUIRE(vtable.points[0].x != vtable.points[26].x);
    REQUIRE(vtable.points[26].x != vtable.points[52].x);

    // UV coords are authored into [0, 1] on both channels.
    for (const auto& uv : vtable.uvCoords)
    {
        REQUIRE(uv.u >= 0.0f);
        REQUIRE(uv.u <= 1.0f);
        REQUIRE(uv.v >= 0.0f);
        REQUIRE(uv.v <= 1.0f);
    }
}

TEST_CASE("sky_plane.p3d: Phase 3 - Bounding Volumes & Textures", "[p3d][sky_plane][phase3]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read header
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    REQUIRE(header.lodCount == 1);

    // Read vertex table
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);

    // Read LOD bounds
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);

    // Hint masks are literal: this model sets both to 16384.
    REQUIRE(bounds.orHints == 16384);
    REQUIRE(bounds.andHints == 16384);

    // The box has to enclose the vertex table it belongs to, on every axis. That
    // is the contract a renderer relies on; the numbers in the file are this
    // corpus's own.
    for (const auto& point : vtable.points)
    {
        REQUIRE(point.x >= bounds.minPos.x);
        REQUIRE(point.y >= bounds.minPos.y);
        REQUIRE(point.z >= bounds.minPos.z);
        REQUIRE(point.x <= bounds.maxPos.x);
        REQUIRE(point.y <= bounds.maxPos.y);
        REQUIRE(point.z <= bounds.maxPos.z);
    }
    REQUIRE(bounds.maxPos.x > bounds.minPos.x);
    REQUIRE(bounds.maxPos.y > bounds.minPos.y);
    REQUIRE(bounds.maxPos.z > bounds.minPos.z);
    REQUIRE(bounds.bCenter.x >= bounds.minPos.x);
    REQUIRE(bounds.bCenter.x <= bounds.maxPos.x);
    REQUIRE(bounds.bRadius > 0.0f);

    // Read textures
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);

    // Hardcoded texture list
    REQUIRE(textures.texturePaths.size() == 1);
    REQUIRE(textures.texturePaths[0] == "data\\synthetic\\surface_00.pac");
}

TEST_CASE("sky_plane.p3d: Phase 4 - LOD Edges, Faces, Sections", "[p3d][sky_plane][phase4]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
    QIStream stream(data.data(), static_cast<int>(data.size()));
    Poseidon::Asset::Formats::BinaryReader reader(stream);

    // Read up to Phase 4
    auto header = Poseidon::Asset::Formats::P3D::readHeader(reader);
    auto vtable = Poseidon::Asset::Formats::P3D::readVertexTable(reader);
    auto bounds = Poseidon::Asset::Formats::P3D::readLODBounds(reader);
    auto textures = Poseidon::Asset::Formats::P3D::readTextures(reader);

    // Phase 4: Edges
    auto edges = Poseidon::Asset::Formats::P3D::readLODEdges(reader);
    REQUIRE(edges.mlodIndex.edges.size() == 45);
    REQUIRE(edges.vertexIndex.edges.size() == 53);

    // Phase 4: Faces
    auto faces = Poseidon::Asset::Formats::P3D::readFaces(reader);
    REQUIRE(faces.count == 77);
    // The section table is addressed in bytes, so this offset is the size of the
    // face block -- derived, not a fixture constant.
    REQUIRE(faces.offsetToSections > 0);
    REQUIRE(faces.faces.size() == 77);

    // Check first face
    const auto& f0 = faces.faces[0];
    REQUIRE(f0.flags == 0);
    REQUIRE(f0.textureIndex == 0);
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
    REQUIRE(sections.sections.size() == 1);

    const auto& s0 = sections.sections[0];
    REQUIRE(s0.faceIndexLowerBound == 0);
    // One section covering the whole face block: its upper bound is that block's
    // byte size, which is also what the face table recorded.
    REQUIRE(s0.faceIndexUpperBound == faces.offsetToSections);
    REQUIRE(s0.material == 0);
    REQUIRE(s0.textureIndex == 0);
    REQUIRE(s0.special == 0);

    // Validate section texture indices
    for (const auto& section : sections.sections)
    {
        if (section.textureIndex >= 0)
        {
            REQUIRE(section.textureIndex < static_cast<int16_t>(textures.texturePaths.size()));
        }
    }
}

TEST_CASE("sky_plane.p3d: Phase 5 - Named Selections", "[p3d][sky_plane][phase5]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
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

    // sky_plane has no named selections
    REQUIRE(namedSelections.selections.size() == 0);
}

TEST_CASE("sky_plane.p3d: Phase 6 - Named Properties", "[p3d][sky_plane][phase6]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
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

    // sky_plane has no named properties
    REQUIRE(namedProperties.properties.size() == 0);
}

TEST_CASE("sky_plane.p3d: Phase 7 - Frames", "[p3d][sky_plane][phase7]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
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

    // sky_plane has no animation frames
    REQUIRE(frames.frames.size() == 0);
}

TEST_CASE("sky_plane.p3d: Phase 8 - LOD End Data", "[p3d][sky_plane][phase8]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
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

    // 0xFFB0C0C0: the authored average colour, written for both the top and the
    // body of the LOD.
    REQUIRE(lodEndData.colorTop == 0xFFB0C0C0u);
    REQUIRE(lodEndData.color == 0xFFB0C0C0u);
    REQUIRE(lodEndData.special == 0);
}

TEST_CASE("sky_plane.p3d: Phase 9 - Proxies", "[p3d][sky_plane][phase9]")
{
    auto data = loadFile(GET_FIXTURE("p3d/sky_plane.p3d"));
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

    // sky_plane has no proxies
    REQUIRE(proxies.proxies.size() == 0);
}
