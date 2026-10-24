#pragma once

// Fixture lookup for the C++ suites.
//
// A fixture that is not in this checkout SKIPs the case that asked for it. It must
// never take the process down: abort() here used to stop the binary at the first
// absent file, so every case registered after it silently never ran and the run
// reported nothing at all (exit 127, no summary). See docs/proteus/FIXTURE-SKIP-1.md
// for which fixture families are deliberately absent and why.

#ifdef _WIN32
#include <Windows.h>
#else
#include <unistd.h>
#include <limits.h>
#ifndef MAX_PATH
#define MAX_PATH PATH_MAX
#if __APPLE__
#include <mach-o/dyld.h>
#endif
#endif
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <filesystem>
#include <set>
#include <string>

#include <catch2/catch_test_macros.hpp>

namespace TestFixtures
{

// Distinct fixtures this run could not find. Deliberately leaked: the atexit
// reporter below runs after function-local statics would already be destroyed.
inline std::set<std::string>& MissingFixtureLog()
{
    static std::set<std::string>* log = new std::set<std::string>();
    return *log;
}

inline void ReportMissingFixtureSummary()
{
    const std::set<std::string>& log = MissingFixtureLog();
    if (log.empty())
    {
        return;
    }
    fprintf(stderr,
            "\n=== %zu fixture file(s) absent from this checkout; the cases that needed them were SKIPPED ===\n",
            log.size());
    for (const std::string& name : log)
    {
        fprintf(stderr, "  %s\n", name.c_str());
    }
    fprintf(stderr, "See docs/proteus/FIXTURE-SKIP-1.md for the category each one belongs to.\n");
}

inline void NoteMissingFixture(const char* filename)
{
    static bool registered = false;
    if (!registered)
    {
        registered = true;
        atexit(ReportMissingFixtureSummary);
    }
    MissingFixtureLog().insert(filename);
}

inline const char* GetExecutableDirectory()
{
    static char exeDir[MAX_PATH] = {0};
    if (exeDir[0] == 0)
    {
#ifdef _WIN32
        char exePath[MAX_PATH];
        GetModuleFileNameA(NULL, exePath, MAX_PATH);
        char* lastSlash = strrchr(exePath, '\\');
        if (lastSlash)
        {
            *lastSlash = '\0';
            strcpy(exeDir, exePath);
        }
#else
        char exePath[MAX_PATH];
#if __APPLE__
        uint32_t len = sizeof(exePath) - 1;
        _NSGetExecutablePath(exePath, &len);
#else
        ssize_t len = readlink("/proc/self/exe", exePath, sizeof(exePath) - 1);
#endif
        if (len > 0)
        {
            exePath[len] = '\0';
            char* lastSlash = strrchr(exePath, '/');
            if (lastSlash)
            {
                *lastSlash = '\0';
                strcpy(exeDir, exePath);
            }
        }
#endif
    }
    return exeDir;
}

inline bool FileExists(const std::filesystem::path& path)
{
    std::error_code ec;
    return std::filesystem::is_regular_file(path, ec);
}

inline std::filesystem::path ResolveFixturePath(const char* filename)
{
    std::filesystem::path exeDir(GetExecutableDirectory());
    std::filesystem::path localFixture = exeDir / "fixtures" / filename;
    if (FileExists(localFixture))
    {
        return localFixture;
    }

    for (std::filesystem::path current = exeDir; !current.empty(); current = current.parent_path())
    {
        std::filesystem::path repoFixture = current / "tests" / "fixtures" / filename;
        if (FileExists(repoFixture))
        {
            return repoFixture;
        }

        if (current == current.root_path())
        {
            break;
        }
    }

    return localFixture;
}

inline const char* GetTestFixturePath(const char* filename)
{
    static std::string path;
    path = ResolveFixturePath(filename).string();
    if (!FileExists(path))
    {
        NoteMissingFixture(filename);
        fprintf(stderr, "\n[fixture] SKIP %s: not in this checkout (looked for %s)\n", filename, path.c_str());
        SKIP("fixture not in this checkout: " << filename);
    }

    return path.c_str();
}

inline const char* GetTempFilePath(const char* filename)
{
    static char path[MAX_PATH];
#ifdef _WIN32
    snprintf(path, sizeof(path), "%s\\temp_%s", GetExecutableDirectory(), filename);
#else
    snprintf(path, sizeof(path), "%s/temp_%s", GetExecutableDirectory(), filename);
#endif
    return path;
}

inline bool FixtureExists(const char* filename)
{
    return FileExists(ResolveFixturePath(filename));
}

inline void ReportMissingFixture(const char* filename)
{
    // ResolveFixturePath, not GetTestFixturePath: the latter SKIPs, which would throw
    // out of this reporter before it printed anything.
    const std::string path = ResolveFixturePath(filename).string();
    NoteMissingFixture(filename);

    fprintf(stderr, "\n============================================\n");
    fprintf(stderr, "ERROR: Test fixture file not found!\n");
    fprintf(stderr, "============================================\n");
    fprintf(stderr, "File: %s\n", filename);
    fprintf(stderr, "Expected location: %s\n", path.c_str());
    fprintf(stderr, "============================================\n\n");
}

inline void CleanupTempFile(const char* filepath)
{
#ifdef _WIN32
    DeleteFileA(filepath);
#else
    remove(filepath);
#endif
}

} // namespace TestFixtures

// GET_FIXTURE resolves the path, so it must be hoisted OUT of a CHECK/REQUIRE
// argument: the skip it raises inside an assertion expression is caught by that
// assertion's exception translation and reported as a failure instead of a skip.
//   const char* fixture = GET_FIXTURE("x");   REQUIRE(load(fixture));
#define GET_FIXTURE(filename) TestFixtures::GetTestFixturePath(filename)
#define GET_TEMP_FILE(filename) TestFixtures::GetTempFilePath(filename)
// Absent fixture == skipped case, for the same reason GET_FIXTURE skips: a test that
// cannot see its input has proved nothing, but it has also proved nothing about the
// rest of the suite, so it must not fail the run.
#define REQUIRE_FIXTURE(filename)                               \
    do                                                          \
    {                                                           \
        if (!TestFixtures::FixtureExists(filename))             \
        {                                                       \
            TestFixtures::ReportMissingFixture(filename);       \
            SKIP("fixture not in this checkout: " << filename); \
        }                                                       \
    } while (0)
