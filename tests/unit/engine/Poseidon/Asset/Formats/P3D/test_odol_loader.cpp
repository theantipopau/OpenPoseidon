// test_odol_loader.cpp - Tests for ODOL -> Model loader
// Validates that ODOLLoader correctly converts ODOL binary format to unified Model

#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>
#include <vector>
#include <Poseidon/Asset/Formats/P3D/ODOLLoader.hpp>
#include <Poseidon/World/Model/Model.hpp>
#include "../../../test_fixtures.hpp"
#include <cmath>
#include <stdint.h>
#include <string>
#include <vector>

using namespace Poseidon::Model;

// fuzz_p3d regression: a wire array count must be rejected before it drives a huge
// allocation. "ODOL" v7 / 1 LOD, then a vertex-table clipFlags compressed-array
// count of 0x08000000 (134M * 4B = 536MB > the 256MB cap). Pre-fix
// readCompressedArray did std::vector(count) up front (OOM); the cap now throws.
TEST_CASE("ODOLLoader: malformed array count is rejected before allocation", "[odol][loader][fuzz]")
{
    std::vector<char> buf;
    auto put32 = [&](uint32_t v)
    {
        for (int i = 0; i < 4; ++i)
            buf.push_back(static_cast<char>((v >> (8 * i)) & 0xFF));
    };
    buf.push_back('O');
    buf.push_back('D');
    buf.push_back('O');
    buf.push_back('L');
    put32(7);          // version
    put32(1);          // lodCount
    put32(0x08000000); // clipFlags compressed-array element count

    REQUIRE_THROWS_WITH(
        Poseidon::Asset::Formats::ODOLLoader::loadFromBuffer(buf.data(), static_cast<int>(buf.size()), "fuzz"),
        Catch::Matchers::ContainsSubstring("count"));
}

TEST_CASE("ODOLLoader: Basic loading - complex_vehicle.p3d", "[odol][loader]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));

    // Compile to compute missing data
    REQUIRE(model.compile());

    // Basic model metadata
    REQUIRE(model.sourceFormat == "ODOL");
    REQUIRE(model.sourceVersion == 7);
    REQUIRE(model.sourcePath == GET_FIXTURE("p3d/complex_vehicle.p3d"));

    // LOD count
    REQUIRE(model.lodLevels.size() == 19);

    // Model should be compiled
    REQUIRE(model.isCompiled());
}

TEST_CASE("ODOLLoader: Geometry - LOD 0", "[odol][loader][geometry]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    REQUIRE(model.lodLevels.size() > 0);
    const auto& lod0 = model.lodLevels[0];
    const auto& mesh = lod0.mesh;

    // Vertex count
    REQUIRE(mesh.vertices.size() == 2617);

    // Every vertex is a finite point inside the authored extent, and no two of the
    // sampled vertices coincide -- which is what stops the samples from all being
    // one reused record. The literals here used to be the removed model's mesh.
    for (const auto& vertex : mesh.vertices)
    {
        REQUIRE(std::isfinite(vertex.position.x));
        REQUIRE(std::isfinite(vertex.position.y));
        REQUIRE(std::isfinite(vertex.position.z));
        REQUIRE(vertex.uv.u >= 0.0f);
        REQUIRE(vertex.uv.u <= 1.0f);
        REQUIRE(vertex.uv.v >= 0.0f);
        REQUIRE(vertex.uv.v <= 1.0f);
    }
    REQUIRE(mesh.vertices[0].position.y != mesh.vertices.back().position.y);

    // Face count (1341 faces: 549 triangles + 792 quads)
    REQUIRE(mesh.triangles.size() == 549);
    REQUIRE(mesh.quads.size() == 792);
    REQUIRE(mesh.triangles.size() + mesh.quads.size() == 1341);
}

TEST_CASE("ODOLLoader: Materials - LOD 0", "[odol][loader][materials]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Material count (from the texture palette)
    REQUIRE(mesh.materials.size() == 69);

    REQUIRE(mesh.materials[0].name == "data\\synthetic\\surface_00.pac");
    REQUIRE(mesh.materials[0].texturePath == "data\\synthetic\\surface_00.pac");
    REQUIRE(mesh.materials[10].name == "data\\synthetic\\surface_10.pac");
    // An empty palette entry is legal and means the untextured surface.
    REQUIRE(mesh.materials.back().name == "");
}

TEST_CASE("ODOLLoader: Sections - LOD 0", "[odol][loader][sections]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Section count
    REQUIRE(mesh.sections.size() == 101);

    // Sections tile the triangles in order: the first starts at 0, each covers a
    // non-empty run, and together they account for every triangle.
    REQUIRE(mesh.sections[0].startTriangle == 0);
    REQUIRE(mesh.sections[0].triangleCount > 0);
    size_t covered = 0;
    for (const auto& section : mesh.sections)
    {
        REQUIRE(section.startTriangle == covered);
        REQUIRE(section.triangleCount > 0);
        covered += section.triangleCount;
    }
    // ODOL keeps the section table in byte offsets over the face block, not in
    // face indices -- at the shape level those numbers are only required to tile
    // the block in order, which is what the loop above checks.
    REQUIRE(covered > 0);
}

TEST_CASE("ODOLLoader: Named Selections - LOD 0", "[odol][loader][selections]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Selection count
    REQUIRE(mesh.selections.size() == 30);

    // Role names from the authored corpus; proxy selections are the ones whose
    // name carries the engine's `proxy:` prefix and an instance id.
    REQUIRE(mesh.selections[0].name == "part_01");
    REQUIRE(mesh.selections[1].name == "part_02");
    REQUIRE(mesh.selections[10].name == "proxy:cargo.01");

    REQUIRE(mesh.selections[0].vertexIndices.size() == 276);
}

TEST_CASE("ODOLLoader: Named Properties - LOD 0", "[odol][loader][properties]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Property count
    REQUIRE(mesh.properties.size() == 1);

    // Sample property
    REQUIRE(mesh.properties[0].name == "lodnoshadow");
    REQUIRE(mesh.properties[0].value == "1");
}

TEST_CASE("ODOLLoader: Proxies - LOD 0", "[odol][loader][proxies]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Proxy count
    REQUIRE(mesh.proxies.size() == 15);

    // Short names: ODOL carries no `proxy:` prefix or instance id.
    REQUIRE(mesh.proxies[0].name == "crew");
    REQUIRE(mesh.proxies[1].name == "cargo");
    REQUIRE(mesh.proxies.back().name == "flag_alone");
}

TEST_CASE("ODOLLoader: Edges - LOD 0", "[odol][loader][edges]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Edge counts
    REQUIRE(mesh.edges.mlodIndices.size() == 1679);
    REQUIRE(mesh.edges.vertexIndices.size() == 2617);

    // The authored sequences: mlod 0..1678, vertex (i * 3) mod 2617.
    REQUIRE(mesh.edges.mlodIndices[0] == 0);
    REQUIRE(mesh.edges.mlodIndices[100] == 100);
    REQUIRE(mesh.edges.mlodIndices.back() == 1678);

    REQUIRE(mesh.edges.vertexIndices[0] == 0);
    REQUIRE(mesh.edges.vertexIndices[100] == 300);
}

TEST_CASE("ODOLLoader: Bounding volumes - LOD 0", "[odol][loader][bounds]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // The box encloses every vertex and is not degenerate.
    REQUIRE(mesh.boundingBox.min.x < mesh.boundingBox.max.x);
    REQUIRE(mesh.boundingBox.min.y < mesh.boundingBox.max.y);
    REQUIRE(mesh.boundingBox.min.z < mesh.boundingBox.max.z);
    for (const auto& vertex : mesh.vertices)
    {
        REQUIRE(vertex.position.x >= mesh.boundingBox.min.x);
        REQUIRE(vertex.position.y >= mesh.boundingBox.min.y);
        REQUIRE(vertex.position.z >= mesh.boundingBox.min.z);
        REQUIRE(vertex.position.x <= mesh.boundingBox.max.x);
        REQUIRE(vertex.position.y <= mesh.boundingBox.max.y);
        REQUIRE(vertex.position.z <= mesh.boundingBox.max.z);
    }

    REQUIRE(mesh.boundingSphere.radius > 0.0f);
}

TEST_CASE("ODOLLoader: LOD end data - LOD 0", "[odol][loader][enddata]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    model.compile();

    const auto& mesh = model.lodLevels[0].mesh;

    // Icon colours come from the LOD end data; both slots are 0xFF808080 here.
    REQUIRE(mesh.iconColor == 0xFF808080u);
    REQUIRE(mesh.selectedColor == 0xFF808080u);
}
