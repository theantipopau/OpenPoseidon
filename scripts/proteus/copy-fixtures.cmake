# copy-fixtures.cmake (FIXTURE-2): post-build fixture staging that tolerates the
# source release's deliberate omissions.
#
# The release omits prebuilt binary fixtures with undocumented provenance
# (tests/fixtures/font, pac, jpg, pbo, rtm and friends; see ASSET_SOURCES.md),
# so a hard `cmake -E copy_directory` per dir fails the whole tests-target
# build on the first missing dir. Copy what exists, warn on what does not;
# tests that need unrestored data abort at runtime via GetTestFixturePath
# (documented, not silent) instead of breaking the build for everyone.
#
# Usage: cmake -DFIXTURES_ROOT=<tests/fixtures> -DDEST_DIR=<.../fixtures>
#        -P scripts/proteus/copy-fixtures.cmake
# The mapping mirrors tests/unit/engine/Poseidon/CMakeLists.txt (single place).

if(NOT DEFINED FIXTURES_ROOT OR NOT DEFINED DEST_DIR)
    message(FATAL_ERROR "copy-fixtures.cmake needs -DFIXTURES_ROOT= and -DDEST_DIR=")
endif()

set(MAPPINGS
    "audio|audio"
    "cfg|cfg"
    "font|font"
    "mlod|mlod"
    "p3d|p3d"
    "paa|texture/paa"
    "pac|texture/pac"
    "jpg|jpg"
    "pbo|pbo"
    "rtm|rtm"
    "wrp|wrp"
    "xml|."
)

foreach(pair IN LISTS MAPPINGS)
    string(REPLACE "|" ";" parts "${pair}")
    list(GET parts 0 srcRel)
    list(GET parts 1 dstRel)
    set(src "${FIXTURES_ROOT}/${srcRel}")
    if(dstRel STREQUAL ".")
        set(dst "${DEST_DIR}")
    else()
        set(dst "${DEST_DIR}/${dstRel}")
    endif()
    if(EXISTS "${src}")
        file(COPY "${src}/" DESTINATION "${dst}")
    else()
        message(WARNING "copy-fixtures: '${src}' absent from source release, skipping")
    endif()
endforeach()
message(STATUS "copy-fixtures: staged into ${DEST_DIR}")
