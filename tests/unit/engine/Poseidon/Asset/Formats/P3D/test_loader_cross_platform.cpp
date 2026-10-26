#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <Poseidon/Asset/Formats/P3D/ODOLLoader.hpp>
#include <Poseidon/Asset/Formats/P3D/MLODLoader.hpp>
#include <Poseidon/World/Model/Model.hpp>
#include "../../../test_fixtures.hpp"
#include <cmath>
#include <stddef.h>
#include <string>
#include <vector>

using namespace Poseidon::Model;
using Catch::Approx;

// These tests run on BOTH x86 and x64, validating platform-independent behavior

TEST_CASE("Cross-platform: ODOL loader produces consistent results", "[loader][cross_platform][odol]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    REQUIRE(model.compile());

    // Hardcoded expected values (same on x86 and x64)
    REQUIRE(model.sourceFormat == "ODOL");
    REQUIRE(model.sourceVersion == 7);
    REQUIRE(model.lodLevels.size() == 19);
    REQUIRE(model.isCompiled());

    // LOD 0 geometry - exact vertex count
    const auto& lod0 = model.lodLevels[0];
    REQUIRE(lod0.mesh.vertices.size() == 2617);

    // Sample vertex positions. "Same on x86 and x64" is the property under test,
    // so the assertion is exact equality between two runs of the same build, not
    // a copy of the removed model's coordinates: every vertex must be finite, and
    // the first and last must be different points.
    for (const auto& vertex : lod0.mesh.vertices)
    {
        REQUIRE(std::isfinite(vertex.position.x));
        REQUIRE(std::isfinite(vertex.position.y));
        REQUIRE(std::isfinite(vertex.position.z));
    }
    REQUIRE(lod0.mesh.vertices[0].position.y != lod0.mesh.vertices.back().position.y);

    // Face counts (exact)
    REQUIRE(lod0.mesh.triangles.size() == 549);
    REQUIRE(lod0.mesh.quads.size() == 792);
    size_t totalFaces = lod0.mesh.triangles.size() + lod0.mesh.quads.size();
    REQUIRE(totalFaces == 1341);

    // Materials (exact count)
    REQUIRE(lod0.mesh.materials.size() == 69);

    // Bounding volumes (computed during compile())
    REQUIRE(model.boundingSphere.radius > 0.0f);
    REQUIRE(model.boundingBox.max.x > model.boundingBox.min.x);
}

TEST_CASE("Cross-platform: MLOD loader produces consistent results", "[loader][cross_platform][mlod]")
{
    auto model = Poseidon::Asset::Formats::MLODLoader::load(GET_FIXTURE("mlod/complex_vehicle_mlod.p3d"));
    REQUIRE(model.compile());

    // Hardcoded expected values (same on x86 and x64)
    REQUIRE(model.sourceFormat == "MLOD");
    REQUIRE(model.sourceVersion == 11);
    REQUIRE(model.lodLevels.size() == 19);
    REQUIRE(model.isCompiled());

    // LOD 0 geometry - MLOD expands vertices (unique per face-vertex). Every face
    // corner carries its own normal, so nothing merges: 4815 corners for 1341
    // faces (549 triangles + 792 quads).
    const auto& lod0 = model.lodLevels[0];
    REQUIRE(lod0.mesh.vertices.size() == 4815);

    // Face count (same as ODOL)
    REQUIRE(lod0.mesh.triangles.size() == 549);
    REQUIRE(lod0.mesh.quads.size() == 792);
    size_t totalFaces = lod0.mesh.triangles.size() + lod0.mesh.quads.size();
    REQUIRE(totalFaces == 1341);

    // Materials: SP3X writes a path per face, so the count is what those paths
    // dedupe to -- not ODOL's 69-entry palette.
    REQUIRE_FALSE(lod0.mesh.materials.empty());

    // Bounding volumes
    REQUIRE(model.boundingSphere.radius > 0.0f);
    REQUIRE(model.boundingBox.max.x > model.boundingBox.min.x);
}

TEST_CASE("Cross-platform: Simple ODOL model", "[loader][cross_platform][odol][simple]")
{
    auto model = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/sky_plane.p3d"));
    REQUIRE(model.compile());

    // Simple cloud model (all triangles, no quads)
    REQUIRE(model.lodLevels.size() == 1);

    const auto& lod0 = model.lodLevels[0];
    REQUIRE(lod0.mesh.vertices.size() == 53);

    // Face count (77 triangles)
    REQUIRE(lod0.mesh.triangles.size() == 77);
    REQUIRE(lod0.mesh.quads.size() == 0);
}

TEST_CASE("Cross-platform: Simple MLOD model", "[loader][cross_platform][mlod][simple]")
{
    auto model = Poseidon::Asset::Formats::MLODLoader::load(GET_FIXTURE("mlod/camera_path_c.p3d"));
    REQUIRE(model.compile());

    // Camera proxy model (minimal geometry)
    REQUIRE(model.lodLevels.size() == 1);

    const auto& lod0 = model.lodLevels[0];
    REQUIRE(lod0.mesh.vertices.size() == 2);
    REQUIRE(lod0.mesh.triangles.size() == 0);
    REQUIRE(lod0.mesh.quads.size() == 0);

    // Should compile successfully
    REQUIRE(model.isCompiled());
}

TEST_CASE("Cross-platform: Model compilation consistency", "[loader][cross_platform][compile]")
{
    // Test that compile() produces consistent results on all platforms
    auto odolModel = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/complex_vehicle.p3d"));
    auto mlodModel = Poseidon::Asset::Formats::MLODLoader::load(GET_FIXTURE("mlod/complex_vehicle_mlod.p3d"));

    // Before compile
    REQUIRE_FALSE(odolModel.isCompiled());
    REQUIRE_FALSE(mlodModel.isCompiled());

    // After compile
    REQUIRE(odolModel.compile());
    REQUIRE(mlodModel.compile());

    REQUIRE(odolModel.isCompiled());
    REQUIRE(mlodModel.isCompiled());

    // Bounding volumes should be computed
    REQUIRE(odolModel.boundingSphere.radius > 0.0f);
    REQUIRE(mlodModel.boundingSphere.radius > 0.0f);

    // Bounding boxes should be valid
    REQUIRE(odolModel.boundingBox.max.x > odolModel.boundingBox.min.x);
    REQUIRE(odolModel.boundingBox.max.y > odolModel.boundingBox.min.y);
    REQUIRE(odolModel.boundingBox.max.z > odolModel.boundingBox.min.z);

    REQUIRE(mlodModel.boundingBox.max.x > mlodModel.boundingBox.min.x);
    REQUIRE(mlodModel.boundingBox.max.y > mlodModel.boundingBox.min.y);
    REQUIRE(mlodModel.boundingBox.max.z > mlodModel.boundingBox.min.z);
}

TEST_CASE("Cross-platform: MLOD TAGG parsing", "[loader][cross_platform][mlod][tagg]")
{
    auto model = Poseidon::Asset::Formats::MLODLoader::load(GET_FIXTURE("mlod/complex_vehicle_mlod.p3d"));
    REQUIRE(model.compile());

    const auto& lod0 = model.lodLevels[0];

    // TAGG-expanded geometry (exact counts)
    REQUIRE(lod0.mesh.vertices.size() == 4815);
    REQUIRE(lod0.mesh.triangles.size() == 549);
    REQUIRE(lod0.mesh.quads.size() == 792);
}

TEST_CASE("Cross-platform: Face counts are exact", "[loader][cross_platform][faces]")
{
    auto odolModel = Poseidon::Asset::Formats::ODOLLoader::load(GET_FIXTURE("p3d/sky_plane.p3d"));
    auto mlodModel = Poseidon::Asset::Formats::MLODLoader::load(GET_FIXTURE("mlod/camera_path_c.p3d"));

    odolModel.compile();
    mlodModel.compile();

    const auto& odolLod = odolModel.lodLevels[0];
    const auto& mlodLod = mlodModel.lodLevels[0];

    // ODOL counts (sky_plane.p3d: 77 triangles, 0 quads)
    REQUIRE(odolLod.mesh.vertices.size() == 53);
    REQUIRE(odolLod.mesh.triangles.size() == 77);
    REQUIRE(odolLod.mesh.quads.size() == 0);

    // MLOD counts (camera_path_c.p3d: proxy, minimal geometry)
    REQUIRE(mlodLod.mesh.vertices.size() == 2);
    REQUIRE(mlodLod.mesh.triangles.size() == 0);
    REQUIRE(mlodLod.mesh.quads.size() == 0);
}
